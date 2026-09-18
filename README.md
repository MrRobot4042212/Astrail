# Astrail

Biblioteca unificada de juegos y aplicaciones para Windows. Detecta automáticamente
tus juegos de **Steam, Epic, GOG, EA, Ubisoft y Xbox (Game Pass)**, te deja añadir
cualquier otra app o juego a mano, y muestra las carátulas de cada uno — todo en una
sola ventana desde la que lanzar lo que quieras.

Stack: **Tauri 2 (Rust)** como núcleo nativo + **Next.js 16 (export estático)** como
frontend. No hay servidor: toda la lógica vive en Rust y se invoca desde React.

---

## Requisitos (Windows)

1. **Node.js 20+** — https://nodejs.org (Next 16 lo exige; CI usa 20)
2. **Rust** (incluye `cargo`) — https://rustup.rs
3. **Microsoft C++ Build Tools** (workload "Desktop development with C++") —
   https://visualstudio.microsoft.com/visual-cpp-build-tools/ — el enlazador y
   el SDK de Windows que usa Rust en Windows (toolchain MSVC).
4. **.NET 8 SDK** — https://dotnet.microsoft.com/download — compila el sidecar
   `cputemp` (temperatura de CPU, métricas y FPS de GPU AMD).
5. **WebView2** — ya viene preinstalado en Windows 11.

## Arrancar en desarrollo

```bash
npm install
powershell -File scripts/fetch-binaries.ps1   # una vez: PresentMon + sidecar
npm run app                                   # = tauri dev
```

`scripts/fetch-binaries.ps1` deja `PresentMon.exe` (descargado y **verificado por
SHA-256**) y `cputemp.exe` (compilado desde `src-tauri/sidecar/cputemp`) en
`src-tauri/binaries/`. Ambos están declarados como recursos en `tauri.conf.json`
y excluidos del repositorio, así que sin ellos `cargo check` y el build fallan.

La primera compilación de Rust tarda un poco; las siguientes son incrementales.

## Compilar el instalador

```bash
npm run app:build  # genera el instalador NSIS en src-tauri/target/release/bundle
```

## Comprobaciones de calidad

```bash
npm run check      # eslint + tsc + vitest + clippy -D warnings + cargo test
```

Portadas: la resolución vía IGDB necesita credenciales **en tiempo de
compilación** (`IGDB_CLIENT_ID` / `IGDB_CLIENT_SECRET`, ver `.env.example`). Sin
ellas la app funciona igual, solo que no resuelve carátulas automáticamente.

---

## Cómo funciona

Cada tienda tiene su propio escáner; si una no está instalada, simplemente no
aporta juegos (no rompe el resto). La lista se mezcla y se deduplica por nombre.

- **Steam**: `steamlocate` recorre las librerías y lee los manifiestos. Arte del
  CDN público de Steam.
- **Epic**: manifiestos `.item` en `%PROGRAMDATA%\Epic\...`. Lanza vía
  `com.epicgames.launcher://`.
- **GOG**: registro (`GOG.com\Games`). Lanza el ejecutable directamente.
- **Ubisoft**: registro (`Ubisoft\Launcher\Installs`). Lanza vía `uplay://`.
- **EA / Origin**: registro (`EA Games` / `Origin Games`); detección best-effort.
- **Xbox / Game Pass**: escanea `XboxGames\...\MicrosoftGame.config` y lanza con
  `gamelaunchhelper.exe`.
- **Apps manuales**: se eligen con el selector de archivos nativo y se guardan en
  `manual_apps.json` dentro de la carpeta de datos de la app.

### Carátulas

Todas las carátulas (de cualquier tienda, Steam incluido) se obtienen de **IGDB**.
Astrail busca por nombre (probando variantes para absorber símbolos y ediciones),
prefiere la coincidencia exacta y usa la portada vertical de IGDB

Las imágenes se **descargan y guardan en disco** (`covers/`) la primera vez, así
que a partir de ahí cargan al instante, sin red y sin volver a llamar a la API
(la URL también se cachea en `cover_cache.json`). Los fallos se reintentan pasado
un tiempo, así que las carátulas que falten no se quedan vacías para siempre.


## Estructura

```
src/                      # Frontend Next.js
  app/                    # layout, page, estilos globales
  components/             # Sidebar, GameCard, AddAppDialog, SettingsDialog, iconos
  hooks/useLibrary.ts     # carga/recarga + resolución de carátulas en 2º plano
  lib/                    # tipos, wrappers de comandos Tauri, metadatos de fuentes
src-tauri/                # Núcleo Rust
  src/
    lib.rs                # comandos Tauri y builder
    models.rs             # Game / GameSource
    steam.rs epic.rs gog.rs ea.rs ubisoft.rs xbox.rs   # un escáner por tienda
    art.rs                # resolución de carátulas vía IGDB (+ caché)
    igdb.rs               # cliente IGDB (auth Twitch OAuth)
    settings.rs           # ajustes (API key de SteamGridDB)
    storage.rs            # persistencia de apps manuales
    launcher.rs           # lanzamiento (protocolo de tienda o ejecutable)
  capabilities/           # permisos (core + dialog)
  tauri.conf.json
```


## Licencia

Astrail es software libre bajo la **GNU General Public License v3** —
[LICENSE](LICENSE) — con los términos adicionales de la sección 7 recogidos en
[ADDITIONAL-TERMS.md](ADDITIONAL-TERMS.md):

1. Conservar la atribución de autoría en *Ajustes → Acerca de* (GPL 7b).
2. Marcar las versiones modificadas como no oficiales (GPL 7c).
3. Sin derechos sobre el nombre «Astrail», «Dalfon.dev» ni el logo (GPL 7e).

Copyright (C) 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev).

Cómo se decide qué entra y quién publica: [GOVERNANCE.md](GOVERNANCE.md). Cómo
contribuir (sin CLA): [CONTRIBUTING.md](CONTRIBUTING.md).

## Créditos

Astrail incluye código de otras personas, cada uno con su licencia. La lista
completa y los textos íntegros están en
[THIRD-PARTY-NOTICES.txt](THIRD-PARTY-NOTICES.txt), y la app también los muestra en
*Ajustes → Acerca de*. Lo más visible:

- [PresentMon](https://github.com/GameTechDev/PresentMon) (Intel, MIT) — FPS y
  frametime por swapchain.
- [LibreHardwareMonitor](https://github.com/LibreHardwareMonitor/LibreHardwareMonitor)
  (MPL-2.0) — temperaturas y telemetría, dentro del sidecar `cputemp`.
- [Tauri](https://tauri.app) (MIT/Apache-2.0), [Next.js](https://nextjs.org) y
  [React](https://react.dev) (MIT).
- Tipografías [Oxanium](https://github.com/sevmeyer/oxanium) y
  [Source Code Pro](https://github.com/adobe-fonts/source-code-pro) (OFL-1.1).

Los nombres y logos de Steam, Epic Games, GOG, EA, Ubisoft, Xbox, Battle.net y
demás tiendas pertenecen a sus dueños y se usan solo para identificarlas. Astrail no
está afiliado a ninguna de ellas.
