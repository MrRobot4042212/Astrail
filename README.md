# Astrail

Biblioteca unificada de juegos y aplicaciones para Windows. Detecta automáticamente
tus juegos de **Steam, Epic, GOG, EA, Ubisoft, Xbox (Game Pass), Battle.net, Riot,
Rockstar, Amazon Games y BattleState Games**, y las apps instaladas en Windows; te
deja añadir cualquier otra app o juego a mano, y muestra las carátulas de cada uno —
todo en una sola ventana desde la que lanzar lo que quieras.

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
npm run check      # eslint + bindings + tsc + vitest + cabeceras y avisos legales + clippy -D warnings + cargo test
```

Portadas: la resolución vía IGDB necesita credenciales **en tiempo de
compilación** (`IGDB_CLIENT_ID` / `IGDB_CLIENT_SECRET`, ver `.env.example`). Sin
ellas la app funciona igual, solo que no resuelve carátulas automáticamente.

---

## Cómo funciona

Las tiendas con datos propios tienen su escáner; las demás se reconocen en la
lista de programas instalados de Windows. Si una tienda no está instalada,
simplemente no aporta juegos (no rompe el resto). Las listas se mezclan y se
deduplican por nombre: la copia de una tienda gana a la genérica del registro.

- **Steam**: `steamlocate` recorre las librerías y lee los manifiestos. Lanza vía
  `steam://rungameid/<id>`.
- **Epic**: manifiestos `.item` en `%PROGRAMDATA%\Epic\EpicGamesLauncher\Data\Manifests`.
  Lanza vía `com.epicgames.launcher://`.
- **GOG**: registro (`GOG.com\Games`). Lanza el ejecutable directamente.
- **Ubisoft**: registro (`Ubisoft\Launcher\Installs`). Lanza vía `uplay://`.
- **EA / Origin**: registro (`EA Games` / `Origin Games`); detección best-effort.
- **Xbox / Game Pass**: paquetes instalados (`Get-AppxPackage`), que se lanzan con
  `shell:appsFolder\<AUMID>`, y carpetas `XboxGames\...\MicrosoftGame.config`, que
  se lanzan con `gamelaunchhelper.exe` o con el ejecutable que declara el config.
- **Battle.net**: entradas de desinstalación de Blizzard en el registro. Lanza vía
  `battlenet://<código>`.
- **Riot, Rockstar, Amazon Games y BattleState Games**: se reconocen en la lista de
  programas instalados por su editor o su carpeta de instalación.
- **Apps de Windows**: el resto de esa lista se clasifica en apps (navegadores,
  ofimática, herramientas…) o en juegos genéricos; runtimes y drivers se descartan.
- **Apps manuales**: se eligen con el selector de archivos nativo y se guardan en
  `manual_apps.json` dentro de la carpeta de datos de la app.

### Carátulas

Todas las carátulas (de cualquier tienda, Steam incluido) se obtienen de **IGDB**.
Astrail busca por nombre (probando variantes para absorber símbolos y ediciones),
prefiere la coincidencia exacta y usa la portada vertical de IGDB. La ficha de cada
juego usa una versión más grande de la misma portada.

Las imágenes se **descargan y guardan en disco** (`covers/`) la primera vez, así
que a partir de ahí cargan al instante, sin red y sin volver a llamar a la API
(el identificador de cada imagen se guarda en `cover_cache.json`, así que la
versión grande no repite la búsqueda). Los fallos se reintentan pasado
un tiempo, así que las carátulas que falten no se quedan vacías para siempre.

La búsqueda se puede apagar con **Buscar carátulas en internet**: en la
configuración inicial, antes del primer escaneo (así no llega a enviarse ningún
nombre), o después en *Ajustes → Preferencias de aplicación*. Apagada, Astrail no
contacta con IGDB: muestra las carátulas que ya tiene guardadas y las que se ponen
a mano, y los demás juegos se quedan sin carátula.


## Estructura

```
src/                      # Frontend Next.js
  app/                    # layout, ventana principal, ventana del HUD (overlay/), fuentes
  components/             # biblioteca, ficha, ajustes, diálogos, iconos
  hooks/                  # biblioteca (carga y carátulas en 2º plano), ajustes, diálogos…
  i18n/                   # textos en español e inglés
  lib/                    # wrappers de comandos Tauri, lógica pura con sus tests
    bindings/             # tipos generados desde Rust (npm run bindings)
src-tauri/                # Núcleo Rust
  src/
    lib.rs                # arranque y builder de Tauri
    commands/             # comandos que invoca el frontend
    models.rs             # Game, GameSource, AppSettings
    steam.rs epic.rs gog.rs ea.rs ubisoft.rs xbox.rs battlenet.rs   # escáneres
    windows_apps.rs apps_db.rs   # programas instalados: otras tiendas, apps, juegos
    library.rs            # mezcla y deduplicación
    art.rs                # resolución de carátulas vía IGDB (+ caché)
    igdb.rs               # cliente IGDB (auth Twitch OAuth)
    storage.rs jsonstore.rs   # datos en JSON, con escritura atómica
    launcher.rs           # lanzamiento (protocolo de tienda o ejecutable)
    playtime.rs           # tiempo de juego
    overlay*.rs metrics.rs presentmon.rs cputemp.rs   # HUD y sus fuentes
    updates.rs            # búsqueda de actualizaciones por canal
  sidecar/cputemp/        # sidecar .NET: temperatura de CPU, métricas y FPS de GPU AMD
  capabilities/           # permisos de cada ventana (main, overlay)
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

## Política de firma de código (Code signing policy)

> El instalador todavía no lleva firma de código de Windows, así que SmartScreen
> puede avisar la primera vez que se abre.

- **Qué se publica:** solo las versiones oficiales. Las compila
  [release.yml](.github/workflows/release.yml) en GitHub Actions a partir de este
  repositorio; no se publica nada compilado en local. Los componentes de otros
  proyectos que ya firman sus autores (PresentMon) conservan su firma original.
- **Actualizaciones:** Astrail comprueba la firma de cada actualización (clave del
  actualizador de Tauri, en los secretos del repositorio) antes de instalarla.
- **Cómo comprobar un instalador:** GitHub publica el SHA-256 de cada archivo de la
  release, y la sección de descarga de [astrail.es](https://astrail.es) da el
  comando de PowerShell que lo compara.
- **Roles** (detalle en [GOVERNANCE.md](GOVERNANCE.md)):
  - Committers and reviewers: [Diego Alfonso Chicoma Ibañez (Dalfon.dev)](https://github.com/MrRobot4042212)
  - Publica las versiones: [Diego Alfonso Chicoma Ibañez (Dalfon.dev)](https://github.com/MrRobot4042212)
- **Cuentas:** quien puede escribir en el repositorio o publicar versiones usa
  autenticación en dos pasos en GitHub.
- **Privacidad:** Astrail no envía datos personales a su autor. Se conecta a IGDB
  (Twitch) para las carátulas, salvo que se apague en Ajustes; a GitHub para buscar
  actualizaciones, y a Discord solo si se activa en Ajustes. Todo lo que envía está
  en la [política de privacidad](https://astrail.es/es/privacy), que el instalador
  resume antes de instalar.

Política completa: [astrail.es/es/code-signing](https://astrail.es/es/code-signing).
Para informar de un instalador que diga ser de Astrail y no venga de una versión
oficial, abre un
[aviso de seguridad privado](https://github.com/MrRobot4042212/Astrail/security/advisories/new).

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
