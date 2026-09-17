//! MPO-friendly in-game HUD via **DirectComposition + DXGI flip swapchain +
//! Direct2D/DirectWrite** (Windows only).
//!
//! Unlike the GDI layered window (`overlay_native`), a DirectComposition visual backed
//! by a flip-model swapchain is the surface type DWM can promote to a **hardware
//! overlay plane (MPO)**. With the overlay on its own plane the game keeps its
//! **independent-flip** path — no per-frame desktop composition, so no added input lag.
//! The window uses `WS_EX_NOREDIRECTIONBITMAP` (no redirection surface) and the
//! swapchain's premultiplied alpha for transparency; it is click-through and topmost.
//!
//! Owned and driven entirely by the metrics sampler thread (the COM objects never
//! cross threads), so all state is a `thread_local`. If any init step fails the facade
//! (`overlay`) falls back to the GDI window, so nothing breaks on odd drivers.


use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use windows::core::{w, Interface, Result, PCWSTR};
use windows::Win32::Foundation::{HMODULE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_RECT_F,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Bitmap1, ID2D1Device, ID2D1DeviceContext, ID2D1Factory1, ID2D1Image,
    ID2D1SolidColorBrush, D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_TARGET,
    D2D1_BITMAP_PROPERTIES1, D2D1_DEVICE_CONTEXT_OPTIONS_NONE, D2D1_DRAW_TEXT_OPTIONS_NONE,
    D2D1_FACTORY_TYPE_SINGLE_THREADED,
};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION,
};
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice, IDCompositionDevice, IDCompositionTarget, IDCompositionVisual,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, DWRITE_FACTORY_TYPE_SHARED,
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_NORMAL,
    DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_MEASURING_MODE_NATURAL, DWRITE_TEXT_ALIGNMENT_LEADING,
    DWRITE_TEXT_ALIGNMENT_TRAILING, DWRITE_TEXT_METRICS,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    IDXGIDevice, IDXGIFactory2, IDXGISurface, IDXGISwapChain1, IDXGISwapChainMedia,
    DXGI_FRAME_STATISTICS_MEDIA, DXGI_PRESENT, DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1,
    DXGI_SWAP_CHAIN_FLAG, DXGI_SWAP_EFFECT_FLIP_DISCARD, DXGI_USAGE_RENDER_TARGET_OUTPUT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, PeekMessageW,
    RegisterClassExW, SetWindowPos,
    ShowWindow, TranslateMessage, HWND_NOTOPMOST, HWND_TOPMOST, MSG, PM_REMOVE, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SW_HIDE, SW_SHOWNOACTIVATE, WNDCLASSEXW, WS_EX_NOACTIVATE,
    WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

use crate::metrics::{MetricsSample, MonitorGeometry};
use crate::models::OverlaySettings;

/// All DirectComposition/D3D/D2D state. Lives only on the sampler thread.
struct Dcomp {
    hwnd: HWND,
    _d3d: ID3D11Device,
    dcomp: IDCompositionDevice,
    _target: IDCompositionTarget,
    _visual: IDCompositionVisual,
    d2d_ctx: ID2D1DeviceContext,
    dwrite: IDWriteFactory,
    brush: ID2D1SolidColorBrush,
    swapchain: IDXGISwapChain1,
    sw_w: i32,
    sw_h: i32,
    last_x: i32,
    last_y: i32,
    visible: bool,
    last_sig: u64,
    /// DirectWrite objects that depend only on the configured sizes, not on the
    /// sampled values. See `TextCache`.
    text_cache: Option<TextCache>,
}

/// Per-frame-invariant DirectWrite state, rebuilt only when the font size or the
/// monitor scale changes.
///
/// The render path used to call `CreateTextFormat` three times and
/// `CreateTextLayout` about eighteen times **per drawn frame**. `CreateTextLayout`
/// runs itemization, shaping and font fallback; it is the most expensive call in
/// the path. Eight of those layouts measured the row labels, which are
/// `&'static str` that never change, and one measured the string `"0"` purely to
/// obtain the line height. All of it depends on `(font_size, scale)` alone.
struct TextCache {
    /// `(label_px, value_px, title_px)` as raw bits — the key that invalidates it.
    key: (u32, u32, u32),
    label: IDWriteTextFormat,
    value: IDWriteTextFormat,
    title: IDWriteTextFormat,
    /// Line height, from measuring a digit once in the value format.
    value_h: f32,
    /// UTF-16 text and measured width per row label, keyed by the label itself.
    labels: std::collections::HashMap<&'static str, (Vec<u16>, f32)>,
}

impl Drop for Dcomp {
    fn drop(&mut self) {
        // The COM interfaces release themselves in field order; the window is
        // ours to destroy. Without this, `hide()` (SW_HIDE) left the HWND, the
        // D3D11 device, the swapchain and the D2D/DWrite objects resident for
        // the rest of the session after a single game.
        if !self.hwnd.is_invalid() {
            // SAFETY: the window was created on this thread and is not in use.
            unsafe {
                let _ = DestroyWindow(self.hwnd);
            }
        }
    }
}

thread_local! {
    static STATE: RefCell<Option<Dcomp>> = const { RefCell::new(None) };
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    DefWindowProcW(hwnd, msg, wp, lp)
}

fn color(rgb: (u8, u8, u8), a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: rgb.0 as f32 / 255.0,
        g: rgb.1 as f32 / 255.0,
        b: rgb.2 as f32 / 255.0,
        a,
    }
}

/// Try to build the whole pipeline. Returns true once and caches the state; false if
/// any step fails (the facade then uses the GDI fallback).
pub fn try_init() -> bool {
    if STATE.with(|s| s.borrow().is_some()) {
        return true;
    }
    match unsafe { init() } {
        Ok(d) => {
            STATE.with(|s| *s.borrow_mut() = Some(d));
            true
        }
        Err(e) => {
            eprintln!("Overlay DirectComposition no disponible, usando GDI: {e}");
            false
        }
    }
}

unsafe fn init() -> Result<Dcomp> {
    // Wrap each fallible DXGI/D3D/DComp call so a failure names the exact step in the
    // overlay-debug log (DXGI_ERROR_INVALID_CALL 0x887A0001 is otherwise opaque about
    // *which* call the driver rejected).
    macro_rules! step {
        ($name:expr, $e:expr) => {
            match $e {
                Ok(v) => v,
                Err(e) => {
                    crate::overlay_diag::log(&format!("init DComp falló en {}: {e}", $name));
                    return Err(e);
                }
            }
        };
    }

    let hinst = step!("GetModuleHandleW", GetModuleHandleW(None));
    let class = w!("MeteorOverlayDComp");
    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(wndproc),
        hInstance: hinst.into(),
        lpszClassName: class,
        ..Default::default()
    };
    RegisterClassExW(&wc);

    // Topmost, click-through, no-activate, no taskbar, and crucially NO redirection
    // bitmap — transparency comes from the composed premultiplied swapchain.
    let hwnd = step!(
        "CreateWindowExW",
        CreateWindowExW(
            WS_EX_NOREDIRECTIONBITMAP
                | WS_EX_TOPMOST
                | WS_EX_TRANSPARENT
                | WS_EX_NOACTIVATE
                | WS_EX_TOOLWINDOW,
            class,
            PCWSTR::null(),
            WS_POPUP,
            0,
            0,
            8,
            8,
            None,
            None,
            Some(hinst.into()),
            None,
        )
    );

    // D3D11 device (BGRA support is required for Direct2D interop).
    let mut d3d: Option<ID3D11Device> = None;
    step!(
        "D3D11CreateDevice",
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut d3d),
            None,
            None,
        )
    );
    let d3d = d3d.ok_or_else(windows::core::Error::from_win32)?;
    let dxgi_device: IDXGIDevice = step!("d3d.cast::<IDXGIDevice>", d3d.cast());

    // DirectComposition device + target for our window + a root visual.
    let dcomp: IDCompositionDevice = step!("DCompositionCreateDevice", DCompositionCreateDevice(&dxgi_device));
    let target = step!("CreateTargetForHwnd", dcomp.CreateTargetForHwnd(hwnd, true));
    let visual = step!("CreateVisual", dcomp.CreateVisual());

    // Composition flip swapchain (premultiplied alpha → transparent HUD background).
    let adapter = step!("dxgi_device.GetAdapter", dxgi_device.GetAdapter());
    let factory2: IDXGIFactory2 = step!("adapter.GetParent::<IDXGIFactory2>", adapter.GetParent());
    let desc = DXGI_SWAP_CHAIN_DESC1 {
        Width: 8,
        Height: 8,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        BufferCount: 2,
        // FLIP_DISCARD lets the runtime drop buffer contents after present (no
        // partial-present constraint that pins composition), keeping the plane
        // MPO-friendly. NOTE: `Scaling` must be DXGI_SCALING_STRETCH here — composition
        // swapchains (CreateSwapChainForComposition) reject DXGI_SCALING_NONE on many
        // drivers (AMD included) with DXGI_ERROR_INVALID_CALL (0x887A0001). With the
        // buffer sized 1:1 to the HUD content there is no actual stretch, so MPO
        // eligibility is unaffected.
        SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
        AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
        Scaling: DXGI_SCALING_STRETCH,
        ..Default::default()
    };
    let swapchain = step!(
        "CreateSwapChainForComposition",
        factory2.CreateSwapChainForComposition(&d3d, &desc, None)
    );
    step!("visual.SetContent", visual.SetContent(&swapchain));
    step!("target.SetRoot", target.SetRoot(&visual));
    step!("dcomp.Commit", dcomp.Commit());

    // Direct2D device context targeting the swapchain back buffer, plus DirectWrite.
    let d2d_factory: ID2D1Factory1 =
        step!("D2D1CreateFactory", D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None));
    let d2d_device: ID2D1Device = step!("d2d_factory.CreateDevice", d2d_factory.CreateDevice(&dxgi_device));
    let d2d_ctx = step!(
        "CreateDeviceContext",
        d2d_device.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)
    );
    d2d_ctx.SetDpi(96.0, 96.0); // we size everything in device px ourselves
    let brush = step!(
        "CreateSolidColorBrush",
        d2d_ctx.CreateSolidColorBrush(&color((255, 255, 255), 1.0), None)
    );
    let dwrite: IDWriteFactory = step!("DWriteCreateFactory", DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED));

    Ok(Dcomp {
        hwnd,
        _d3d: d3d,
        dcomp,
        _target: target,
        _visual: visual,
        d2d_ctx,
        dwrite,
        brush,
        swapchain,
        sw_w: 8,
        sw_h: 8,
        last_x: i32::MIN,
        last_y: i32::MIN,
        visible: false,
        last_sig: 0,
        text_cache: None,
    })
}

/// Logical font sizes (label, value) per key; title fixed at 10.
fn font_sizes(key: &str) -> (f32, f32) {
    match key {
        "xs" => (9.0, 12.0),
        "base" => (12.0, 16.0),
        _ => (10.0, 14.0),
    }
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

impl Dcomp {
    unsafe fn make_format(&self, px: f32, semibold: bool, trailing: bool) -> Result<IDWriteTextFormat> {
        let weight = if semibold {
            DWRITE_FONT_WEIGHT_SEMI_BOLD
        } else {
            DWRITE_FONT_WEIGHT_NORMAL
        };
        let fmt = self.dwrite.CreateTextFormat(
            w!("Consolas"),
            None,
            weight,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            px,
            w!("en-us"),
        )?;
        fmt.SetTextAlignment(if trailing {
            DWRITE_TEXT_ALIGNMENT_TRAILING
        } else {
            DWRITE_TEXT_ALIGNMENT_LEADING
        })?;
        Ok(fmt)
    }

    unsafe fn measure(&self, fmt: &IDWriteTextFormat, text: &[u16]) -> (f32, f32) {
        match self.dwrite.CreateTextLayout(text, fmt, f32::MAX, f32::MAX) {
            Ok(layout) => {
                let mut m = DWRITE_TEXT_METRICS::default();
                if layout.GetMetrics(&mut m).is_ok() {
                    return (m.width, m.height);
                }
                (0.0, 0.0)
            }
            Err(_) => (0.0, 0.0),
        }
    }

    /// Ensure `text_cache` matches these sizes, rebuilding it only when they move.
    ///
    /// Returns the three formats (a clone is a COM `AddRef`, not a creation) and
    /// the cached line height, so the caller holds no borrow of `self` and can go
    /// on to call `resize`.
    unsafe fn text_state(
        &mut self,
        label_px: f32,
        value_px: f32,
        title_px: f32,
        labels: &[&'static str],
    ) -> Result<(IDWriteTextFormat, IDWriteTextFormat, IDWriteTextFormat, f32)> {
        let key = (
            label_px.to_bits(),
            value_px.to_bits(),
            title_px.to_bits(),
        );
        if self.text_cache.as_ref().map(|c| c.key) != Some(key) {
            let label = self.make_format(label_px, false, false)?;
            let value = self.make_format(value_px, true, true)?;
            let title = self.make_format(title_px, true, false)?;
            let (_, value_h) = self.measure(&value, &to_wide("0"));
            self.text_cache = Some(TextCache {
                key,
                label,
                value,
                title,
                value_h,
                labels: std::collections::HashMap::new(),
            });
        }

        // Measure any label not seen yet at this size. The set is small and fixed
        // (one entry per metric row), so this converges after the first frame.
        let missing: Vec<&'static str> = {
            let cache = match self.text_cache.as_ref() {
                Some(c) => c,
                None => return Err(windows::core::Error::empty()),
            };
            labels
                .iter()
                .copied()
                .filter(|l| !cache.labels.contains_key(l))
                .collect()
        };
        for l in missing {
            let text = to_wide(l);
            let fmt = match self.text_cache.as_ref() {
                Some(c) => c.label.clone(),
                None => return Err(windows::core::Error::empty()),
            };
            let (w, _) = self.measure(&fmt, &text);
            if let Some(c) = self.text_cache.as_mut() {
                c.labels.insert(l, (text, w));
            }
        }

        let cache = match self.text_cache.as_ref() {
            Some(c) => c,
            None => return Err(windows::core::Error::empty()),
        };
        Ok((
            cache.label.clone(),
            cache.value.clone(),
            cache.title.clone(),
            cache.value_h,
        ))
    }

    /// UTF-16 text and width for a cached row label.
    fn cached_label(&self, label: &'static str) -> Option<(Vec<u16>, f32)> {
        self.text_cache
            .as_ref()
            .and_then(|c| c.labels.get(label))
            .map(|(t, w)| (t.clone(), *w))
    }

    unsafe fn resize(&mut self, w: i32, h: i32) -> Result<()> {
        if w == self.sw_w && h == self.sw_h {
            return Ok(());
        }
        // Release any back-buffer reference before resizing, or ResizeBuffers returns
        // DXGI_ERROR_INVALID_CALL (0x887A0001).
        self.d2d_ctx.SetTarget(None::<&ID2D1Image>);
        self.swapchain.ResizeBuffers(
            2,
            w as u32,
            h as u32,
            DXGI_FORMAT_B8G8R8A8_UNORM,
            DXGI_SWAP_CHAIN_FLAG(0),
        )?;
        self.sw_w = w;
        self.sw_h = h;
        Ok(())
    }

    unsafe fn draw(&self, w: i32, h: i32, bg: &D2D1_COLOR_F, items: &[DrawItem]) -> Result<()> {
        let surface: IDXGISurface = self.swapchain.GetBuffer(0)?;
        let props = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
            bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
            colorContext: std::mem::ManuallyDrop::new(None),
        };
        let bitmap: ID2D1Bitmap1 = self.d2d_ctx.CreateBitmapFromDxgiSurface(&surface, Some(&props))?;
        self.d2d_ctx.SetTarget(&bitmap);
        self.d2d_ctx.BeginDraw();
        self.d2d_ctx.Clear(Some(&color((0, 0, 0), 0.0)));

        // Background panel.
        let full = D2D_RECT_F { left: 0.0, top: 0.0, right: w as f32, bottom: h as f32 };
        self.brush.SetColor(bg);
        self.d2d_ctx.FillRectangle(&full, &self.brush);

        for it in items {
            match it {
                DrawItem::Rect { rect, col } => {
                    self.brush.SetColor(col);
                    self.d2d_ctx.FillRectangle(rect, &self.brush);
                }
                DrawItem::Text { text, fmt, rect, col } => {
                    self.brush.SetColor(col);
                    self.d2d_ctx.DrawText(
                        text,
                        fmt,
                        rect,
                        &self.brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );
                }
            }
        }

        self.d2d_ctx.EndDraw(None, None)?;
        // Release the back-buffer target before Present / the next ResizeBuffers, else
        // the held reference triggers DXGI_ERROR_INVALID_CALL.
        self.d2d_ctx.SetTarget(None::<&ID2D1Image>);
        // Sync to vblank; on its own MPO plane this costs the game nothing.
        self.swapchain.Present(1, DXGI_PRESENT(0)).ok()?;
        Ok(())
    }
}

enum DrawItem {
    Rect { rect: D2D_RECT_F, col: D2D1_COLOR_F },
    Text { text: Vec<u16>, fmt: IDWriteTextFormat, rect: D2D_RECT_F, col: D2D1_COLOR_F },
}

/// Render the HUD. Returns false on a hard failure so the facade can fall back to GDI.
pub fn render(cfg: &OverlaySettings, m: &MetricsSample, monitor: MonitorGeometry) -> bool {
    STATE.with(|s| {
        let mut guard = s.borrow_mut();
        let Some(d) = guard.as_mut() else { return false };
        match unsafe { render_inner(d, cfg, m, monitor) } {
            Ok(()) => true,
            Err(e) => {
                eprintln!("Overlay DComp render falló: {e}");
                false
            }
        }
    })
}

unsafe fn render_inner(
    d: &mut Dcomp,
    cfg: &OverlaySettings,
    m: &MetricsSample,
    monitor: MonitorGeometry,
) -> Result<()> {
    let scale = monitor.scale;
    let (title_str, rows) = crate::overlay::build_rows(cfg, m);
    if rows.is_empty() {
        hide();
        return Ok(());
    }

    let accent = crate::overlay::parse_rgb(&cfg.accent_color);
    let label_rgb = crate::overlay::parse_rgb(&cfg.label_color);

    // Present-on-change: skip the whole frame if nothing visible changed.
    let sig = {
        let mut h = DefaultHasher::new();
        title_str.hash(&mut h);
        for r in &rows {
            r.label.hash(&mut h);
            r.value.hash(&mut h);
            r.rgb.hash(&mut h);
        }
        cfg.position.hash(&mut h);
        cfg.font_size.hash(&mut h);
        cfg.bg_opacity.hash(&mut h);
        cfg.accent_color.hash(&mut h);
        cfg.label_color.hash(&mut h);
        (monitor.left, monitor.top, monitor.width, monitor.height).hash(&mut h);
        (scale.to_bits()).hash(&mut h);
        h.finish()
    };
    if d.visible && sig == d.last_sig {
        return Ok(());
    }

    let s = scale.max(1.0) as f32;
    let (label_px, value_px) = font_sizes(&cfg.font_size);
    let pad_x = (12.0 * s).round();
    let pad_y = (8.0 * s).round();
    let gap = (16.0 * s).round();
    let row_gap = (2.0 * s).round();
    let title_gap = (6.0 * s).round();
    let div_gap = (4.0 * s).round();
    let min_w = (148.0 * s).round();

    // Formats, the line-height probe and the static label widths all depend only
    // on (font_size, scale), so they are built once per config change instead of
    // once per frame.
    let labels: Vec<&'static str> = rows.iter().map(|r| r.label).collect();
    let (label_fmt, value_fmt, title_fmt, value_h) =
        d.text_state(label_px * s, value_px * s, 10.0 * s, &labels)?;

    // Measure. Only the values are measured per frame — they are the only part
    // that actually changes between ticks.
    let mut inner_w = 0.0f32;
    let title_wide = title_str.as_deref().map(to_wide);
    let mut title_h = 0.0f32;
    if let Some(t) = &title_wide {
        let (tw, th) = d.measure(&title_fmt, t);
        title_h = th;
        inner_w = inner_w.max(tw);
    }
    let mut measured: Vec<(Vec<u16>, Vec<u16>)> = Vec::with_capacity(rows.len());
    for r in &rows {
        let (lw_text, lw) = match d.cached_label(r.label) {
            Some(v) => v,
            // Not cached (an unexpected label): fall back to measuring it.
            None => {
                let text = to_wide(r.label);
                let (w, _) = d.measure(&label_fmt, &text);
                (text, w)
            }
        };
        let vw_text = to_wide(&r.value);
        let (vw, _) = d.measure(&value_fmt, &vw_text);
        inner_w = inner_w.max(lw + gap + vw);
        measured.push((lw_text, vw_text));
    }

    // Quantized to a 16 px step. The raw width moves whenever the widest value
    // gains or loses a digit (99 → 100 fps, 9.9 → 10.0 GB), and every change calls
    // `ResizeBuffers`, which reallocates the swapchain buffers and can knock the
    // HUD off its MPO hardware plane mid-game.
    const WIDTH_STEP: i32 = 16;
    let raw_w = (inner_w + pad_x * 2.0).max(min_w).ceil() as i32;
    let w = (raw_w + WIDTH_STEP - 1) / WIDTH_STEP * WIDTH_STEP;
    let title_block = if title_wide.is_some() {
        title_h + title_gap + 1.0 + div_gap
    } else {
        0.0
    };
    let rows_h = value_h * rows.len() as f32 + row_gap * (rows.len() as f32 - 1.0).max(0.0);
    let h = (pad_y * 2.0 + title_block + rows_h).ceil() as i32;

    // Position the window in the chosen corner.
    let margin = (12.0 * s).round() as i32;
    let (x, yy) = crate::overlay::hud_origin(&cfg.position, monitor, w, h, margin);
    if x != d.last_x || yy != d.last_y || w != d.sw_w || h != d.sw_h {
        let _ = SetWindowPos(d.hwnd, Some(HWND_TOPMOST), x, yy, w, h, SWP_NOACTIVATE);
        d.last_x = x;
        d.last_y = yy;
    }
    d.resize(w, h)?;

    // Build the draw list.
    let mut items: Vec<DrawItem> = Vec::new();
    let mut y = pad_y;
    if let Some(t) = &title_wide {
        items.push(DrawItem::Text {
            text: t.clone(),
            fmt: title_fmt.clone(),
            rect: D2D_RECT_F { left: pad_x, top: y, right: w as f32 - pad_x, bottom: y + title_h },
            col: color(accent, 1.0),
        });
        y += title_h + title_gap;
        items.push(DrawItem::Rect {
            rect: D2D_RECT_F { left: pad_x, top: y, right: w as f32 - pad_x, bottom: y + 1.0 },
            col: color((0x2a, 0x2a, 0x2a), 1.0),
        });
        y += 1.0 + div_gap;
    }
    for (i, r) in rows.iter().enumerate() {
        let (lw_text, vw_text) = &measured[i];
        let rect = D2D_RECT_F { left: pad_x, top: y, right: w as f32 - pad_x, bottom: y + value_h };
        items.push(DrawItem::Text {
            text: lw_text.clone(),
            fmt: label_fmt.clone(),
            rect,
            col: color(label_rgb, 1.0),
        });
        items.push(DrawItem::Text {
            text: vw_text.clone(),
            fmt: value_fmt.clone(),
            rect,
            col: color(r.rgb, 1.0),
        });
        y += value_h + row_gap;
    }

    let bg = color((0, 0, 0), (cfg.bg_opacity.min(100) as f32) / 100.0);
    d.draw(w, h, &bg, &items)?;
    d.dcomp.Commit()?;

    if !d.visible {
        let _ = ShowWindow(d.hwnd, SW_SHOWNOACTIVATE);
        d.visible = true;
    }
    d.last_sig = sig;
    Ok(())
}

/// Hide the HUD window.
pub fn hide() {
    STATE.with(|s| {
        if let Some(d) = s.borrow_mut().as_mut() {
            if d.visible {
                unsafe {
                    let _ = ShowWindow(d.hwnd, SW_HIDE);
                }
                d.visible = false;
                d.last_x = i32::MIN;
                d.last_y = i32::MIN;
                d.last_sig = 0;
            }
        }
    });
}

/// Drop the whole DirectComposition stack (window included).
///
/// Called by the sampler after a stretch with no game: keeping a D3D11 device
/// and a flip swapchain alive to draw nothing costs GPU and system memory. The
/// window class stays registered, so a later `try_init` just recreates the
/// window.
pub fn teardown() {
    STATE.with(|s| {
        s.borrow_mut().take();
    });
}

/// Re-insert at the front of the topmost band after a foreground change.
pub fn reassert_topmost() {
    STATE.with(|s| {
        if let Some(d) = s.borrow().as_ref() {
            let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
            unsafe {
                let _ = SetWindowPos(d.hwnd, Some(HWND_NOTOPMOST), 0, 0, 0, 0, flags);
                let _ = SetWindowPos(d.hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, flags);
            }
        }
    });
}

/// Raw DXGI composition mode of the HUD swapchain (0=COMPOSED, 1=OVERLAY, 2=NONE,
/// 3=COMPOSITION_FAILURE), or `None` if the backend isn't initialized, the HUD isn't
/// visible (no presented frames to measure), or the query fails. This is the runtime
/// MPO signal the sampler uses to know whether the overlay is free or costing the game.
pub fn composition_mode() -> Option<i32> {
    STATE.with(|s| {
        let guard = s.borrow();
        let d = guard.as_ref()?;
        if !d.visible {
            return None;
        }
        let media: IDXGISwapChainMedia = d.swapchain.cast().ok()?;
        let mut stats = DXGI_FRAME_STATISTICS_MEDIA::default();
        if unsafe { media.GetFrameStatisticsMedia(&mut stats) }.is_ok() {
            Some(stats.CompositionMode.0)
        } else {
            None
        }
    })
}

/// Query and log the swapchain's actual composition mode — the definitive runtime
/// MPO check. `OVERLAY` = the HUD is on a hardware overlay plane (the game keeps
/// independent-flip → no added latency); `COMPOSED` = DWM is compositing it (the
/// input-lag case). Diagnostics-gated, so this only runs with METEOR_OVERLAY_DEBUG.
pub fn log_composition_mode() {
    STATE.with(|s| {
        let guard = s.borrow();
        let Some(d) = guard.as_ref() else {
            crate::overlay_diag::log("composición: swapchain DComp no inicializado");
            return;
        };
        if !d.visible {
            crate::overlay_diag::log("composición: HUD oculto (sin frames que medir)");
            return;
        }
        let media: Result<IDXGISwapChainMedia> = d.swapchain.cast();
        match media {
            Ok(m) => {
                let mut stats = DXGI_FRAME_STATISTICS_MEDIA::default();
                match unsafe { m.GetFrameStatisticsMedia(&mut stats) } {
                    Ok(()) => {
                        let mode = stats.CompositionMode.0;
                        crate::overlay_diag::log(&format!(
                            "composición swapchain: {} (CompositionMode={mode}, approvedPresentDuration={})",
                            crate::overlay_diag::composition_mode_name(mode),
                            stats.ApprovedPresentDuration
                        ));
                    }
                    Err(e) => crate::overlay_diag::log(&format!(
                        "GetFrameStatisticsMedia falló: {e} (frecuente justo tras mostrar; reintenta el siguiente heartbeat)"
                    )),
                }
            }
            Err(e) => {
                crate::overlay_diag::log(&format!("IDXGISwapChainMedia no disponible: {e}"))
            }
        }
    });
}

/// Drain pending window messages for the HUD window.
pub fn pump() {
    STATE.with(|s| {
        if s.borrow().is_some() {
            unsafe {
                let mut msg = MSG::default();
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        }
    });
}
