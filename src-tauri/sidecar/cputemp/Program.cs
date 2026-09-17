// Hardware sidecar for Meteor's metrics overlay (built as cputemp.exe).
//
//   --cpu        CPU package temperature. LibreHardwareMonitor reads it (Ryzen
//                Tctl/Tdie, Intel core) through a kernel driver it loads at runtime,
//                so this needs admin; without it no temperature sensor appears and
//                the key is simply left out.
//   --gpu <sel>  AMD GPU telemetry and FPS, read through AMD's display library
//                (ADL) that ships with the graphics driver. No admin and no kernel
//                driver of ours, elevated or not (see AmdGpus). <sel> is "auto" (the
//                AMD GPU with the most memory) or a fragment of the adapter's PnP
//                instance id, e.g. VEN_1002&DEV_7550&SUBSYS_88111EAE&REV_C0.
//   No flag = --cpu, for running it by hand.
//
// Protocol: one line per second on stdout, space-separated key=value pairs in
// invariant culture. A key whose sensor has no reading is left out, and the line
// may be empty. That is all the Rust controller (cputemp.rs) parses:
//
//   cpu_temp=54 gpu_usage=9 gpu_temp=44 gpu_power=27.4 gpu_clock=152 vram_used=4340 vram_total=16304 fps=144
//
// Shutdown protocol: the parent closes our stdin, we see EOF, call computer.Close()
// and exit. This matters more than it looks — with --cpu, Open() makes
// LibreHardwareMonitor load a kernel driver and only Close() unloads it (with
// --gpu, Close() stops ADL's frame-metrics and power logging). Being
// TerminateProcess'd (which is what a kill or the parent's kill-on-close Job
// Object does) skips .NET finalizers, so the driver would stay loaded and
// registered for the rest of the boot. That residue is what vulnerable-driver
// blocklists and kernel anti-cheats look for, so it must not depend on the happy
// path alone.

using System.Globalization;
using System.Reflection;
using System.Text;
using LibreHardwareMonitor.Hardware;
using LibreHardwareMonitor.Hardware.Gpu;

var cpu = false;
string? gpu = null;
for (var i = 0; i < args.Length; i++)
{
    switch (args[i])
    {
        // Checks the sensor picking without opening any hardware, so CI (no admin,
        // no GPU) can exercise it.
        case "--self-test":
            return SelfTest.Run();
        case "--cpu":
            cpu = true;
            break;
        case "--gpu" when i + 1 < args.Length:
            gpu = args[++i];
            break;
        default:
            Console.Error.WriteLine("cputemp: unknown argument " + args[i]);
            return 2;
    }
}
if (!cpu && gpu is null) cpu = true;

Computer? computer = null;
AmdGpus.Group? amdGroup = null;

// Unload the driver exactly once, whichever path we leave by.
var closed = 0;
void Shutdown()
{
    if (Interlocked.Exchange(ref closed, 1) != 0) return;
    try { computer?.Close(); }
    catch (Exception e) { Console.Error.WriteLine("cputemp: close failed: " + e.Message); }
    try { amdGroup?.Close(); }
    catch (Exception e) { Console.Error.WriteLine("cputemp: gpu close failed: " + e.Message); }
}

try
{
    if (cpu)
    {
        computer = new Computer { IsCpuEnabled = true };
        computer.Open();
    }
    if (gpu is not null) amdGroup = AmdGpus.Open();
}
catch (Exception e)
{
    Console.Error.WriteLine("cputemp: open failed: " + (e.InnerException ?? e).Message);
    Shutdown();
    return 1;
}

// Covers a normal return and an unhandled exception; TerminateProcess still cannot
// be intercepted, which is why the parent asks over stdin instead of killing.
AppDomain.CurrentDomain.ProcessExit += (_, _) => Shutdown();

using var stop = new ManualResetEventSlim(false);

Console.CancelKeyPress += (_, e) =>
{
    // Handle it ourselves so the sampling loop can unwind through Shutdown().
    e.Cancel = true;
    stop.Set();
};

// EOF on stdin = the parent dropped its write handle and wants us gone.
new Thread(() =>
{
    try { Console.In.ReadToEnd(); }
    catch { /* closed underneath us; treat as a stop request */ }
    stop.Set();
})
{ IsBackground = true, Name = "stdin-watch" }.Start();

var cpus = computer?.Hardware.Where(h => h.HardwareType == HardwareType.Cpu).ToList() ?? new List<IHardware>();
var gpuHw = gpu is null || amdGroup is null ? null : GpuPick.Select(amdGroup.Hardware, gpu);
if (gpu is not null && gpuHw is null)
    Console.Error.WriteLine("cputemp: no AMD GPU matches " + gpu);

var line = new StringBuilder();
void Put(string key, float? value, string format)
{
    if (value is not float v) return;
    if (line.Length > 0) line.Append(' ');
    line.Append(key).Append('=').Append(v.ToString(format, CultureInfo.InvariantCulture));
}

while (!stop.IsSet)
{
    line.Clear();

    // Only the hardware we report on is updated: the rest of the tree would cost a
    // driver round trip per sensor for numbers nobody reads.
    float? cpuTemp = null;
    foreach (var hw in cpus)
    {
        Hw.Refresh(hw);
        cpuTemp = SensorRank.Pick(Hw.Readings(hw, SensorType.Temperature).Select(r => (r.Name, r.Value)));
        if (cpuTemp is not null) break;
    }
    Put("cpu_temp", cpuTemp, "0");

    if (gpuHw is not null)
    {
        Hw.Refresh(gpuHw);
        var g = GpuSensors.Pick(Hw.Readings(gpuHw));
        Put("gpu_usage", g.Usage, "0");
        Put("gpu_temp", g.Temp, "0");
        Put("gpu_power", g.Power, "0.0");
        Put("gpu_clock", g.Clock, "0");
        Put("vram_used", g.VramUsed, "0");
        Put("vram_total", g.VramTotal, "0");
        Put("fps", g.Fps, "0");
    }

    Console.WriteLine(line.ToString());
    Console.Out.Flush();

    // Wait, but wake immediately when the parent asks us to stop, so a shutdown
    // never has to sit through the rest of a sampling second.
    if (stop.Wait(1000)) break;
}

Shutdown();
return 0;

static class Hw
{
    public static void Refresh(IHardware hw)
    {
        hw.Update();
        foreach (var sub in hw.SubHardware) Refresh(sub);
    }

    // Current sensor readings of one piece of hardware (optionally of one type),
    // skipping sensors without a value.
    public static IEnumerable<(SensorType Type, string Name, float Value)> Readings(IHardware hw, SensorType? type = null)
    {
        foreach (var s in hw.Sensors)
        {
            if (type is SensorType t && s.SensorType != t) continue;
            if (s.Value is not float v || float.IsNaN(v)) continue;
            yield return (s.SensorType, s.Name ?? string.Empty, v);
        }
    }
}

// Picks the CPU temperature from one CPU's temperature sensors.
//
// Ordered preference, first hit per rank. The previous loop let the *last* sensor
// whose name contained Package/Tctl/Tdie win, so on multi-CCD Ryzen a per-CCD
// sensor could displace the package reading depending on enumeration order.
// Sensor names follow LibreHardwareMonitor's naming (reported, not verified against
// every LHM version): Intel "CPU Package" / "Core Max" / "CPU Core #n"; AMD
// "Core (Tctl/Tdie)", "Core (Tdie)", "Core (Tctl)", "CCDs Max (Tdie)", "CCD1 (Tdie)".
static class SensorRank
{
    // Lower is better; null = never use (averages, TjMax distances).
    public static int? Rank(string name)
    {
        if (name.Contains("Distance") || name.Contains("Average")) return null;
        if (name.Contains("Package")) return 0;
        bool ccd = name.Contains("CCD");
        if (!ccd && name.Contains("Tdie") && !name.Contains("Tctl")) return 1;
        if (!ccd && name.Contains("Tctl")) return 2;
        if (name.Contains("CCDs Max")) return 3;
        if (name.Contains("Core Max")) return 4;
        if (ccd || name.Contains("Core")) return 5;
        return null;
    }

    public static float? Pick(IEnumerable<(string Name, float Value)> readings)
    {
        int bestRank = int.MaxValue;
        float? best = null;
        foreach (var (name, value) in readings)
        {
            // Without the driver (no admin) LHM still lists the sensors, reading 0.
            if (value <= 0) continue;
            if (Rank(name) is not int r || r > bestRank) continue;
            if (r < bestRank) { bestRank = r; best = value; }
            // Individual cores/CCDs: report the hottest; better ranks keep the first hit.
            else if (r == 5 && best is float b && value > b) best = value;
        }
        return best;
    }
}

// What the overlay shows for a GPU. Units: %, °C, W, MHz, MB, frames per second.
readonly record struct GpuReading(
    float? Usage, float? Temp, float? Power, float? Clock,
    float? VramUsed, float? VramTotal, float? Fps);

// Picks the overlay's GPU values from one AMD GPU's sensors. Names as
// LibreHardwareMonitor 0.9.7-pre704 reports them on an RX 9070 XT and a Ryzen
// iGPU, without admin.
static class GpuSensors
{
    public static GpuReading Pick(IEnumerable<(SensorType Type, string Name, float Value)> readings)
    {
        var all = readings.ToList();
        float? Find(SensorType type, params string[] names)
        {
            foreach (var name in names)
                foreach (var r in all)
                    if (r.Type == type && r.Name == name) return r.Value;
            return null;
        }

        // The driver's own memory counters when present; the D3D view otherwise.
        var used = Find(SensorType.SmallData, "GPU Memory Used");
        var total = Find(SensorType.SmallData, "GPU Memory Total");
        if (used is null || total is null)
        {
            used = Find(SensorType.SmallData, "D3D Dedicated Memory Used");
            total = Find(SensorType.SmallData, "D3D Dedicated Memory Total");
        }

        // -1 while no fullscreen application is presenting.
        var fps = Find(SensorType.Factor, "Fullscreen FPS");

        return new GpuReading(
            Usage: Find(SensorType.Load, "GPU Core", "D3D 3D"),
            // Integrated GPUs have no core temperature; their "GPU VR SoC" is the
            // voltage regulator's, so nothing stands in for it.
            Temp: Find(SensorType.Temperature, "GPU Core"),
            Power: Find(SensorType.Power, "GPU Package", "GPU PPT", "GPU Socket", "GPU Core"),
            Clock: Find(SensorType.Clock, "GPU Core"),
            VramUsed: used,
            VramTotal: total,
            Fps: fps > 0 ? fps : null);
    }
}

// Opens only LibreHardwareMonitor's AMD GPU group. Computer.Open() would also run
// Ring0.Open(), SMBIOS parsing and the CPU probes of the Intel GPU group, and
// Ring0.Open() installs LHM's kernel driver whenever the process is elevated
// (Meteor run as admin) — for numbers ADL gives without it. The group type is
// internal, hence the reflection; --self-test resolves it, so a
// LibreHardwareMonitorLib update that renames it fails CI instead of the HUD.
static class AmdGpus
{
    public sealed class Group(object instance, MethodInfo close, IReadOnlyList<IHardware> hardware)
    {
        public IReadOnlyList<IHardware> Hardware { get; } = hardware;
        public void Close() => close.Invoke(instance, null);
    }

    // Null when this LibreHardwareMonitorLib no longer has the members used here.
    public static (ConstructorInfo Ctor, PropertyInfo Hardware, MethodInfo Close)? Resolve()
    {
        var type = Type.GetType("LibreHardwareMonitor.Hardware.Gpu.AmdGpuGroup, LibreHardwareMonitorLib");
        var group = Type.GetType("LibreHardwareMonitor.Hardware.IGroup, LibreHardwareMonitorLib");
        var ctor = type?.GetConstructor(BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic, new[] { typeof(ISettings) });
        var hardware = group?.GetProperty("Hardware");
        var close = group?.GetMethod("Close");
        if (type is null || group is null || ctor is null || hardware is null || close is null || !group.IsAssignableFrom(type))
            return null;
        return (ctor, hardware, close);
    }

    public static Group Open()
    {
        var (ctor, hardware, close) = Resolve()
            ?? throw new InvalidOperationException("LibreHardwareMonitorLib has no AmdGpuGroup");
        var instance = ctor.Invoke(new object[] { new NoSettings() });
        var list = (IReadOnlyList<IHardware>?)hardware.GetValue(instance) ?? Array.Empty<IHardware>();
        return new Group(instance, close, list);
    }

    // The group only asks settings for per-sensor overrides; there are none.
    sealed class NoSettings : ISettings
    {
        public bool Contains(string name) => false;
        public void SetValue(string name, string value) { }
        public string GetValue(string name, string value) => value;
        public void Remove(string name) { }
    }
}

static class GpuPick
{
    // The AMD GPU to report on: the one whose PnP instance id contains `selector`,
    // or for "auto" the one with the most memory (a discrete card over the iGPU).
    public static IHardware? Select(IEnumerable<IHardware> hardware, string selector)
    {
        var amd = hardware.Where(h => h.HardwareType == HardwareType.GpuAmd).ToList();
        if (selector != "auto")
            return amd.FirstOrDefault(h =>
                h is GenericGpu g && g.DeviceId.Contains(selector, StringComparison.OrdinalIgnoreCase));

        IHardware? best = null;
        float bestTotal = -1;
        foreach (var hw in amd)
        {
            Hw.Refresh(hw);
            var total = GpuSensors.Pick(Hw.Readings(hw, SensorType.SmallData)).VramTotal ?? 0;
            if (total > bestTotal) { best = hw; bestTotal = total; }
        }
        return best;
    }
}

static class SelfTest
{
    public static int Run()
    {
        var failures = 0;
        void Check<T>(string label, T got, T want)
        {
            if (EqualityComparer<T>.Default.Equals(got, want)) return;
            Console.Error.WriteLine($"FAIL {label}: got {got}, want {want}");
            failures++;
        }

        Check("LibreHardwareMonitorLib still has AmdGpuGroup", AmdGpus.Resolve() is not null, true);

        var pick = SensorRank.Pick;
        Check("package beats a later CCD", pick(new[] { ("CPU Package", 60f), ("CCD1 (Tdie)", 70f) }), 60f);
        Check("package beats an earlier CCD", pick(new[] { ("CCD2 (Tdie)", 72f), ("Core (Tctl/Tdie)", 65f), ("CCD1 (Tdie)", 70f) }), 65f);
        Check("Tdie beats Tctl", pick(new[] { ("Core (Tctl)", 75f), ("Core (Tdie)", 65f) }), 65f);
        Check("CCDs Max beats one CCD", pick(new[] { ("CCD1 (Tdie)", 61f), ("CCDs Max (Tdie)", 68f), ("CCDs Average (Tdie)", 64f) }), 68f);
        Check("hottest individual core", pick(new[] { ("CPU Core #1", 50f), ("CPU Core #2", 58f), ("CPU Core #3", 55f) }), 58f);
        Check("TjMax distance is not a temperature", pick(new[] { ("CPU Core #1 Distance to TjMax", 45f), ("CPU Core #1", 55f) }), 55f);
        Check("averages are ignored", pick(new[] { ("Core Average", 50f) }), null);
        Check("no sensors", pick(Array.Empty<(string, float)>()), null);
        Check("unread sensors (no admin) are left out", pick(new[] { ("Core (Tctl/Tdie)", 0f), ("CCD1 (Tdie)", 0f) }), null);

        // Sensor set of an RX 9070 XT with no fullscreen application.
        var dgpu = GpuSensors.Pick(new[]
        {
            (SensorType.Temperature, "GPU Core", 44f),
            (SensorType.Temperature, "GPU Hot Spot", 45f),
            (SensorType.Load, "GPU Core", 9f),
            (SensorType.Load, "D3D 3D", 3f),
            (SensorType.Power, "GPU Package", 27.4f),
            (SensorType.Clock, "GPU Core", 152f),
            (SensorType.Clock, "GPU Memory", 909f),
            (SensorType.SmallData, "D3D Dedicated Memory Used", 4100f),
            (SensorType.SmallData, "GPU Memory Used", 4340f),
            (SensorType.SmallData, "GPU Memory Total", 16304f),
            (SensorType.Factor, "Fullscreen FPS", -1f),
        });
        Check("dGPU", dgpu, new GpuReading(9f, 44f, 27.4f, 152f, 4340f, 16304f, null));

        // Ryzen iGPU: no core temperature, power split into core and SoC, only D3D memory.
        var igpu = GpuSensors.Pick(new[]
        {
            (SensorType.Temperature, "GPU VR SoC", 41f),
            (SensorType.Load, "D3D 3D", 2f),
            (SensorType.Power, "GPU SoC", 3f),
            (SensorType.Power, "GPU Core", 1.5f),
            (SensorType.SmallData, "D3D Dedicated Memory Used", 120f),
            (SensorType.SmallData, "D3D Dedicated Memory Total", 512f),
        });
        Check("iGPU", igpu, new GpuReading(2f, null, 1.5f, null, 120f, 512f, null));

        Check("FPS of a fullscreen game", GpuSensors.Pick(new[] { (SensorType.Factor, "Fullscreen FPS", 143f) }).Fps, 143f);
        Check("used memory is not paired with another source's total",
            GpuSensors.Pick(new[] { (SensorType.SmallData, "GPU Memory Used", 4340f), (SensorType.SmallData, "D3D Dedicated Memory Total", 16304f) }),
            new GpuReading(null, null, null, null, null, 16304f, null));

        Console.WriteLine(failures == 0 ? "cputemp self-test: ok" : $"cputemp self-test: {failures} failure(s)");
        return failures == 0 ? 0 : 1;
    }
}
