# Changelog

Todas las novedades destacables de **Meteor** (launcher de escritorio que unifica
juegos y apps de varias tiendas en una sola biblioteca).

El formato sigue, a grandes rasgos, [Keep a Changelog](https://keepachangelog.com/es-ES/)
y el proyecto usa versionado semántico aproximado. Las fechas son orientativas.

---

## [No publicado] — Trabajo en curso

### Añadido
- **Meteor es software libre: GPL-3.0-only.** Se publica el texto de la licencia
  ([LICENSE](LICENSE)) y los términos adicionales de la sección 7
  ([ADDITIONAL-TERMS.md](ADDITIONAL-TERMS.md)): conservar la atribución de autoría
  en «Acerca de», marcar las versiones modificadas como no oficiales, y sin
  derechos sobre el nombre ni el logo. No hay CLA.
- **Nueva pestaña «Acerca de» en Ajustes.** Muestra la versión, la autoría con el
  enlace al repositorio y el texto íntegro de la licencia, de los términos
  adicionales y de los avisos de terceros, que van embebidos en el ejecutable. Si
  la compilación no sale del workflow de publicación del proyecto, lo advierte.
- **`THIRD-PARTY-NOTICES.txt`**: los avisos de todo el código de terceros que
  Meteor incluye (313 crates de Rust, 30 paquetes npm, 11 de NuGet, además de
  PresentMon, el runtime de .NET, los módulos de PawnIO y las tipografías). Lo
  genera `scripts/third-party-notices.mjs` a partir de los ficheros de bloqueo, y
  la CI falla si se queda desactualizado.
- **Cabecera SPDX en todos los ficheros de código.** `scripts/check-headers.mjs`
  la verifica (y la añade con `--fix`); `npm run check:legal` corre ambas
  comprobaciones y forma parte de `npm run check` y de la CI.
- **[GOVERNANCE.md](GOVERNANCE.md) y `.github/CODEOWNERS`**: quién decide, quién
  publica y dónde se reportan los fallos de seguridad.

### Cambiado
- **Las métricas de GPU AMD ya no usan el SDK de ADLX.** El sidecar `cputemp`
  (LibreHardwareMonitor) lee uso, temperatura, potencia, reloj, VRAM y FPS de la
  GPU AMD por ADL, la librería que instala el propio driver, sin permisos de
  administrador. Se retiran el SDK de ADLX, su shim en C++ y la dependencia
  `cc`, cuya licencia no permitía publicar Meteor como software libre.
  - **Los FPS en AMD pasan a necesitar administrador** (PresentMon), igual que
    en NVIDIA. ADL solo da FPS de juegos en pantalla completa exclusiva y no los
    da en juegos actuales en ventana sin bordes; cuando los da, se usan.
  - El panel lista todas las GPU por DXGI (también Intel, sin métricas) y
    distingue integrada de dedicada. Las AMD se eligen por su id de PnP, así que
    una GPU elegida en una versión anterior (`adlx:<i>`) vuelve a "Automática".
  - El sidecar se arranca solo con un juego abierto y el overlay pidiendo GPU o
    FPS, y actualiza una vez por segundo.
- **Las carátulas pegadas como URL se descargan a disco.** Al poner una URL como
  carátula (en el diálogo de carátula o al añadir una app manual) Meteor la
  descarga a `user_covers/` y guarda la ruta local; las URLs guardadas por
  versiones anteriores se migran al arrancar. Así la ventana solo carga imágenes
  del CDN de IGDB y de su propio disco, nunca de un servidor arbitrario.
- **Permisos de ventana al mínimo.** La ventana principal solo puede escuchar
  eventos, abrir el diálogo de archivo, comprobar/descargar/instalar
  actualizaciones y relanzarse; la ventana de ajustes in-game solo escuchar
  eventos. Se retiran los permisos de bandeja y menús que ningún código usaba.
- **Iniciar con Windows sin el plugin de autoarranque.** Meteor escribe él mismo
  la clave `Run` del usuario, con la ruta entre comillas, y respeta la casilla
  de "Aplicaciones de inicio" del Administrador de tareas. El desinstalador
  borra ambas entradas.
- **La caché de iconos de apps usa nombres estables y tiene tope.** Los `.ico`
  extraídos se nombran con el mismo hash que las carátulas, se escriben de forma
  atómica y `app_icons/` se mantiene por debajo de 64 MB; los iconos con el
  nombre antiguo se descartan una vez y se vuelven a extraer al vuelo.
- **El sidecar de temperatura ya no deja restos en `%TEMP%`.** Se publica como un
  único fichero sin bibliotecas nativas que extraer; el desinstalador limpia la
  carpeta que dejaban las versiones anteriores.

### Corregido
- **Las apps manuales con el mismo nombre que un juego detectado ya no
  desaparecen**, y los favoritos, categorías, carátulas y tipo asignados a una
  copia duplicada (por ejemplo la del registro) se conservan cuando la copia de
  la tienda pasa a ser la mostrada.
- **Con Meteor abierto como administrador, "Abrir carpeta" y los enlaces externos
  ya no heredan los privilegios**: se abren a través del Explorador del
  escritorio, igual que los juegos.

## [0.1.3] — 2026-09-17

### Cambiado
- **Iniciar con Windows usa siempre la clave `Run` del usuario.** Con Meteor
  abierto como administrador se intentaba crear una tarea programada elevada, que
  en la instalación normal fallaba siempre y dejaba el interruptor roto. Ahora no
  hay tarea: Meteor arranca sin privilegios al iniciar sesión, y la tarea antigua
  se elimina la próxima vez que Meteor se abra como administrador.
- **La publicación se niega a salir sin las credenciales de IGDB.** Antes una
  release sin esos secretos se publicaba igual, con las carátulas desactivadas.
- **Los sidecars elevados se verifican antes de arrancar.** `PresentMon.exe` y
  `cputemp.exe` viven en una carpeta escribible por el usuario; ahora se comprueba
  su SHA-256 contra el de la compilación y el fichero queda bloqueado hasta que el
  proceso existe. Un binario distinto no se ejecuta. Además PresentMon debe estar
  firmado por Intel al empaquetarlo y la restauración NuGet del sidecar de
  temperatura falla si no coincide con el fichero de bloqueo.

### Corregido
- **FPS y temperatura congelados en el HUD.** Si el juego dejaba de presentar
  frames (pausa, cambio de ventana) o el sidecar de temperatura dejaba de
  responder, el overlay seguía mostrando el último valor como si fuera actual.
  Ahora las lecturas caducan (FPS a los 2 s o dos intervalos, temperatura a los
  3 s) y la fila desaparece en lugar de mentir.
- **`CPU 0%` en el primer refresco.** La primera lectura de CPU no tiene con qué
  compararse y salía como 0 %; tras un rato sin juego, la primera medía toda la
  pausa. Ahora la fila no aparece hasta tener una medición real, y la medición se
  reinicia al volver de reposo.
- **FPS inflados en juegos con varias ventanas de render** (interfaces Chromium
  incrustadas, ventanas secundarias): los frames de todas se sumaban. Ahora se
  mide cada swapchain por separado y se muestra la más activa.
- **Frametime inventado en AMD.** Con ADLX el frametime se calculaba dividiendo
  1000 entre un FPS entero y se mostraba con un decimal, como si fuera medido. Esa
  fila ya no aparece en AMD; el FPS sí.
- **HUD en el monitor equivocado.** Con varios monitores, el overlay tomaba el
  tamaño y la escala del monitor del juego pero se colocaba en el principal.
  Ahora se posiciona dentro del monitor del juego, también si está a la izquierda
  o por encima del principal.
- **Sensor de temperatura de CPU equivocado en Ryzen con varios CCD.** Ganaba el
  último sensor que cuadrara por nombre, así que un CCD podía desplazar al valor
  de paquete según el orden. Ahora hay un orden de preferencia fijo (paquete,
  Tdie, Tctl, máximo de CCD, máximo de núcleo, núcleo más caliente) y se ignoran
  medias y distancias a TjMax.
- **El sidecar de temperatura se relanzaba cada medio segundo** cuando no podía
  cargar su driver (HVCI, lista de bloqueo de drivers): un proceso elevado de
  13 MB arrancando dos veces por segundo toda la partida. Ahora espera 30 s antes
  de reintentar.
- **El HUD o el FPS desaparecían en juegos con launcher propio.** Si el juego no
  coincidía exactamente con su ejecutable, se elegía el primer proceso de la
  carpeta de instalación, a menudo el launcher, y el overlay se ocultaba porque
  ese proceso no está en primer plano. Ahora gana el proceso en primer plano, el
  seguimiento cambia a él en unos 5 s, y cerrar el launcher ya no da por
  terminada la sesión de juego.
- **Escrituras constantes en disco durante la partida.** El tiempo de juego en
  curso se guardaba cada 5 s aunque sólo cambiara la hora de última actividad
  (unas 720 escrituras sincronizadas por hora). Ahora se guarda al cambiar las
  sesiones o cada 30 s; tras un cierre inesperado se pueden perder hasta 30 s.
- **Reintentos de NVML/ADLX en cada refresco.** En un equipo sin GPU NVIDIA (o sin
  AMD) se intentaba cargar la librería que falta en cada tick del HUD. Ahora el
  fallo se recuerda hasta que el overlay se libera por inactividad.
- **Cancelar con Esc registraba Esc como atajo global.** Al grabar un atajo, Esc
  se guardaba como combinación en vez de cancelar, y Meteor capturaba la tecla
  Escape en todo el sistema, también dentro de los juegos. Ahora Esc cancela, los
  atajos exigen Ctrl, Alt o Win (Mayús sólo vale con F1–F24) y un atajo guardado
  que no cumpla esto deja de registrarse.
- **Arrastrar el selector de color del HUD escribía los ajustes decenas de veces
  por segundo** (disco, atajos re-registrados, Discord). Ahora la vista previa se
  actualiza al arrastrar y el color se aplica al cerrar el selector.
- **Actualizar dejaba el driver de temperatura y la sesión ETW de PresentMon
  cargados.** El instalador cierra Meteor sin pasar por el apagado normal; ahora
  ambos sidecars se detienen antes de instalar (y vuelven si la instalación falla).
- **Se podían abrir dos Meteor a la vez**, duplicando icono de bandeja, atajos y
  vigilancia de juegos, y el segundo cortaba el FPS del primero. Ahora abrir
  Meteor otra vez trae al frente la ventana existente.
- **Los juegos heredaban permisos de administrador.** Con Meteor elevado (para las
  métricas que lo requieren), cualquier juego o tienda lanzado desde él arrancaba
  también como administrador. Ahora el lanzamiento se delega en el escritorio de
  Windows y el juego arranca con los permisos normales del usuario; si eso no es
  posible, el lanzamiento falla en lugar de elevarlo.
- **Lanzar un juego congelaba Meteor un instante**, también el HUD y los atajos:
  la biblioteca se leía entera en el hilo principal. Ahora se resuelve en segundo
  plano.
- **Cambios de ajustes perdidos entre ventanas.** Cada cambio reescribía todos los
  ajustes a partir de una copia que podía estar desfasada, así que tocar algo en
  la pantalla del overlay deshacía lo cambiado en la ventana principal. Ahora cada
  cambio sólo modifica su campo, y la pantalla del overlay se refresca al cambiar
  los ajustes.
- **La pantalla de ajustes del overlay se abría en el monitor principal** aunque
  el juego estuviera en otro. Ahora cubre el monitor del juego, y ya no difumina
  el fondo encima de la partida.
- **Arrancar PresentMon para nada en AMD con Meteor elevado.** En cada partida se
  abría y cerraba una sesión ETW antes de que ADLX diera su primer FPS; ahora se
  espera a ADLX.
- **La tecla Espacio salía como «Espacio» en inglés** en los atajos, y el idioma
  de la página no seguía al elegido en ajustes.

---

## [0.1.2] — 2026-09-09

### Cambiado
- **Publicar es ahora mergear en la rama `deploy`** (`release.yml`): antes se
  publicaba empujando una etiqueta `v*` a mano. Ahora la versión se lee del
  proyecto, la etiqueta la crea la automatización — así no puede discrepar de lo
  que se ha compilado — y la publicación se niega a salir si los cinco ficheros
  de versión no coinciden entre sí o si esa versión ya está publicada. Esto
  último es la red contra mergear sin subir la versión: republicar la misma
  versión no llega a nadie, porque el actualizador nunca va hacia atrás.

### Corregido
- **La detección de cambios en la biblioteca no funcionaba en perfiles sin
  instalaciones por usuario.** La huella que decide si hace falta un rescaneo
  trataba una clave de registro *inexistente* (`HKCU\...\Uninstall`, que sólo
  aparece tras instalar algo por usuario) como si fuera ilegible y metía la hora
  actual en el hash, así que nunca coincidía consigo misma y Meteor rescaneaba
  las ocho tiendas cada 15 minutos aunque no hubiera cambiado nada. Ahora una
  clave ausente es un estado estable, igual que una carpeta ausente; sólo un
  error real de permisos sigue forzando el rescaneo. Fue lo que hizo fallar
  `fingerprint_is_stable_between_calls` en los runners de GitHub y bloqueó la
  publicación.

### Rendimiento
- **Instalador 1,8 MB más pequeño** (`tauri.conf.json`): incrustaba el instalador
  de WebView2, que **igualmente necesita conexión a internet** para funcionar y
  cuya única ventaja era Windows 7, una versión que la aplicación no soporta.
- **Los comandos pesados ya no pueden dejar la aplicación sin atender**
  (`lib.rs`): marcar un comando como asíncrono en Tauri **no** lo saca del grupo
  de hilos de trabajo cuando su cuerpo es bloqueante — lo ejecuta tal cual dentro
  de uno de ellos, y solo hay tantos como núcleos. Bastaban unas cuantas
  búsquedas de carátula simultáneas (hasta 6 s de conexión más 8 s de lectura,
  por tres variantes de nombre) para que no quedara ningún hilo libre y todo lo
  demás — la biblioteca en caché, los ajustes, la respuesta al lanzar un juego —
  se quedara en cola. Ahora el escaneo, las carátulas, los iconos, el tamaño de
  carpeta y la comprobación de cambios se ejecutan en el grupo de hilos de
  bloqueo, que crece bajo demanda. Además hay un límite propio de 4 búsquedas de
  carátula simultáneas dentro de la aplicación, en vez de depender solo del de la
  interfaz.
- **El escaneo de Game Pass ya no lanza PowerShell cada vez** (`xbox.rs`,
  `fingerprint.rs`): enumerar los paquetes instalados arranca un proceso de
  PowerShell y consulta todos los paquetes del sistema, y eso ocurría en cada
  escaneo — con el refresco de 15 minutos, casi un centenar de procesos al día.
  Ahora el resultado se reutiliza mientras la huella de la biblioteca no cambie,
  y esa huella incluye `WindowsApps`, que es donde se instalan buena parte de los
  juegos de Game Pass y que antes no se vigilaba.
- **El HUD deja de rehacer su tipografía en cada fotograma**
  (`overlay_dcomp.rs`): en cada dibujado creaba los tres formatos de texto y
  medía dieciocho cadenas, ocho de las cuales son etiquetas fijas que no cambian
  nunca, más una medición del alto de línea. Todo eso depende solo del tamaño de
  fuente y de la escala del monitor, así que ahora se calcula una vez y se
  reutiliza. El ancho del HUD además se redondea a múltiplos de 16 píxeles: antes
  cambiaba al pasar de 99 a 100 FPS o de 9,9 a 10,0 GB, y cada cambio reasignaba
  los búferes de la ventana, lo que puede sacar al HUD de su plano de hardware en
  mitad de la partida.
- **La ventana del overlay ya no carga el launcher entero** (`lib.rs`,
  `src/app/overlay/`): ambas ventanas compartían documento y decidían qué mostrar
  al arrancar, así que para dibujar un panel de ajustes se descargaba y evaluaba
  todo el launcher — la rejilla, la pantalla de inicio, el buscador — **mientras
  el juego está corriendo**. Ahora cada ventana tiene su propio documento. Medido
  sobre el paquete compilado: el overlay pasa de 855 880 a 786 976 bytes de
  JavaScript inicial, y el launcher de 855 880 a 841 151.
- **Pulsar Jugar ya no congela la interfaz** (`playtime.rs`): el vigilante de
  procesos mantenía tomado el cerrojo de "juegos lanzados desde Meteor" durante
  todo su ciclo — una enumeración completa de procesos, la reescritura de
  `playtime.json` con su volcado a disco y una llamada a Discord. `launch_game`
  corre en el hilo principal y espera en ese mismo cerrojo, así que el clic se
  quedaba detrás de todo eso, y con él la bandeja, los atajos globales y la
  comunicación con la interfaz. Ahora el vigilante copia la lista y suelta el
  cerrojo de inmediato.
- **La rejilla de la biblioteca deja de reconstruirse por cosas que no le
  incumben** (`page.tsx`, nuevo `LibraryGrid.tsx`): estaba escrita dentro del
  componente que guarda ~25 estados sin relación entre sí, así que un aviso
  emergente, arrastrar una tarjeta, abrir o cerrar el menú contextual o **cada
  pulsación de tecla** en el buscador reconstruían un elemento por juego y
  comparaban sus propiedades una a una. Con 1 000 juegos eso son 1 000 elementos
  y 14 000 comparaciones por cada uno de esos eventos. Extraída y memoizada.
- **Menos trabajo por fotograma leyendo los FPS** (`presentmon.rs`): el lector
  reservaba memoria nueva por cada línea recibida, y se recibe una por fotograma
  presentado — entre 200 y 800 veces por segundo durante toda la partida. Ahora
  reutiliza un único búfer.
- **Un corte de red ya no encadena esperas** (`igdb.rs`): la petición del token de
  Twitch se hacía con el cerrojo tomado, así que con la red caída cada búsqueda
  de carátula esperaba su turno para agotar su propio tiempo de espera, una
  detrás de otra. Ahora la petición ocurre fuera del cerrojo y, si falla, se
  pausan los intentos 30 segundos.
- **Meteor en la bandeja ya no hace prácticamente nada** (`playtime.rs`,
  `metrics.rs`, `cputemp.rs`, `presentmon.rs`): los cuatro hilos que despertaban
  por temporizador ahora **se aparcan** hasta que hay algo que hacer. El watcher
  de tiempo de juego bloquea en un `Condvar` (lo despierta el lanzamiento de un
  juego) en vez de enumerar **todos los procesos del sistema cada 5 s** para
  descartarlos; el sampler del overlay usa `MsgWaitForMultipleObjectsEx` con
  espera infinita mientras el overlay está apagado o no hay juego; el
  controlador de `cputemp` sale de inmediato si no hay permisos de administrador
  (como ya hacía PresentMon) y también se aparca.
- **Cero escrituras en disco en reposo**: `active_sessions.json` se escribía cada
  5 segundos (≈17 000 veces al día) con el mismo `[]`. Ahora solo se escribe si
  el contenido cambia, igual que `hidden_cache.json` y los ajustes (que se
  guardaban en cada pulsación del atajo del overlay).
- **NVML y ADLX se cargan solo cuando hacen falta** (`metrics.rs`): antes se
  cargaban `nvml.dll` y `amdadlx64.dll` al arrancar aunque el overlay estuviese
  desactivado. Ahora se inicializan en el primer dibujado y se liberan tras 60 s
  sin juego, junto con **toda la pila DirectComposition del HUD** (ventana,
  dispositivo D3D11, swapchain), que antes quedaba residente el resto de la
  sesión (`overlay_dcomp.rs`: nuevo `Drop` + `teardown`).
- **WebView2 libera memoria al minimizar a la bandeja** (`lib.rs`): 10 s después
  de ocultar la ventana se le pide `SetMemoryUsageTargetLevel(LOW)`, y se
  restaura al mostrarla.
- **El rescaneo periódico deja de trabajar a ciegas** (`fingerprint.rs`,
  `useLibrary.ts`): antes lanzaba 8 escáneres + un proceso de PowerShell cada 15
  minutos aunque la ventana estuviese oculta. Ahora el temporizador se pausa
  mientras la ventana no se ve (evento `window-visibility` desde Rust) y, antes
  de escanear, un nuevo comando `library_changed` comprueba por marcas de tiempo
  de carpetas y claves del registro si hay algo nuevo.
- **El HUD ya no salta al hilo principal en cada fotograma** (`metrics.rs`): la
  geometría del monitor se lee con `MonitorFromWindow` + `GetDpiForMonitor` en el
  propio hilo del sampler. De paso, el HUD se coloca en el monitor del juego en
  vez de siempre en el principal.
- **Comandos con E/S fuera del hilo principal**: 35 comandos pasan a
  `#[tauri::command(async)]`. Un escaneo de biblioteca ya no congela IPC, la
  bandeja, los atajos globales ni el sampler del HUD.
- **La caché de portadas deja de reescribirse entera por cada portada**
  (`art.rs`): `RwLock` sin clonar el mapa, guardado agrupado (máx. 1 escritura
  cada 2 s) y **límite de 200 MB** con purga por uso; antes crecía sin tope.
- **Portadas del tamaño correcto**: la rejilla usa `t_cover_big` (264×374, lo que
  cabe en una tarjeta de 240 px) y la ficha pide `t_cover_big_2x` solo para su
  cabecera. Antes todo eran imágenes 528×748, ~4× los bytes descodificados.
- **La rejilla vuelve a memoizar** (`page.tsx`, `GameCard.tsx`): los manejadores
  pasan por `useCallback`, así que cambiar un estado cualquiera de `MainApp` ya
  no re-renderiza todas las tarjetas. Además las tarjetas dejan de tener
  `will-change: transform` permanente, montan los botones con desenfoque y el
  borde animado **solo al pasar el ratón**, y usan `content-visibility: auto`.
- **Las portadas se aplican por lotes** (`useLibrary.ts`): un `setState` cada
  120 ms en vez de dos por portada resuelta, y el merge del refresco pasa de
  O(n²) a un `Map`.
- **`get_library` rellena las portadas ya descargadas**, así que un refresco no
  vuelve a pedirlas una por una por IPC; y ordena con clave precalculada en vez
  de dos `to_lowercase()` por comparación.
- **Arranque más corto**: la ventana se crea oculta y se muestra tras el primer
  pintado (sin destello blanco), el idioma ya no bloquea el primer render, la
  intro pasa de 2 s a 0,85 s, y los diálogos, el tour y la ficha se cargan bajo
  demanda (`next/dynamic`).
- **Discord con reintentos escalonados** (5 s → 5 min) y un aviso único, en vez
  de abrir una tubería cada 5 segundos indefinidamente con Discord cerrado.
- **Sidecar de temperatura: 69,3 MB → 12,9 MB** (`cputemp.csproj`), comprimido y
  con recorte parcial; LibreHardwareMonitor y sus dependencias reflexivas quedan
  intactas.

### Seguridad
- **Credenciales de IGDB fuera del binario** (`igdb.rs`): el cliente/secreto de
  Twitch ya no está incrustado como constante de reserva; se leen **solo** de
  `IGDB_CLIENT_ID` / `IGDB_CLIENT_SECRET` en tiempo de compilación (secretos de
  CI o un `.env` local; ver `.env.example`). Un binario compilado sin ellas
  simplemente no resuelve portadas de IGDB (avisa una vez) en lugar de repartir
  la misma credencial a todos los usuarios. **Rota el secreto anterior en
  dev.twitch.tv: estuvo publicado en el repositorio.**
- **Lanzamiento de juegos sin `cmd`** (`launcher.rs`, `lib.rs`): se elimina
  `cmd /C start "" <uri>` —que entregaba la URI al intérprete de comandos— y se
  sustituye por `ShellExecuteW` con **lista blanca de esquemas** (`steam`,
  `com.epicgames.launcher`, `uplay`, `battlenet`, y `shell:appsFolder\` para
  Xbox). Los ejecutables se validan (`canonicalize`, extensión permitida y,
  cuando la entrada tiene `install_dir`, obligados a estar dentro de él).
- **Los comandos reciben ids, no structs** (`lib.rs`, `src/lib/tauri.ts`):
  `launch_game`, `user_screenshots`, `game_dir_size` (antes `dir_size`) y
  `open_game_folder` (antes `open_path`) resuelven la entrada en Rust desde la
  caché de biblioteca o la tienda manual. La webview ya no puede fabricar rutas
  ni URIs de lanzamiento, ni pedir el tamaño o abrir una carpeta arbitraria.
- **Sidecars en un Job Object** (`jobobj.rs`, `presentmon.rs`, `cputemp.rs`):
  `PresentMon.exe` y `cputemp.exe` (elevados, con sesión ETW y driver de kernel)
  se asignan a un Job con `KILL_ON_JOB_CLOSE`, así que el kernel los termina si
  Meteor muere de cualquier forma. Sustituye al `taskkill /F /PID` guardado en
  un atómico, que perdía la carrera con la reutilización de PID y no se
  ejecutaba en caso de cierre forzado o `panic = "abort"`.
- **Sin administrador permanente** (`hooks.nsi`, `elevation.rs`, `lib.rs`): el
  instalador ya no ofrece marcar `RUNASADMIN` (lo elimina si lo puso una versión
  anterior) y la tarea de inicio `/RL HIGHEST` no se crea si el ejecutable está
  en una carpeta escribible por el usuario; en ese caso se migra a la clave
  `Run` normal. Un lanzador siempre elevado pasaba su token a **cada juego**.
- **CSP real y capacidades por ventana** (`tauri.conf.json`, `capabilities/`):
  se sustituye `"csp": null` por una política explícita (con `devCsp` relajada
  solo para el servidor de desarrollo) y `default.json` se divide en
  `main.json` y `overlay.json`; la ventana de overlay se queda con
  `core:default` únicamente.
- **Rutas de sistema absolutas y validación de carpetas** (`files.rs`,
  `xbox.rs`, `elevation.rs`): `explorer.exe`, `schtasks.exe` y `powershell.exe`
  se resuelven desde `%SystemRoot%` y nunca por `PATH` (el proceso puede estar
  elevado); `dir_size` limita profundidad y número de entradas, y la
  enumeración AppX de Xbox tiene un **timeout de 10 s** con caída al escaneo de
  carpetas.
- **PresentMon verificado por SHA-256** (`scripts/fetch-binaries.ps1`,
  `release.yml`) y **NuGet bloqueado** (`packages.lock.json`) para el sidecar,
  que carga un driver de kernel.
- **`parse_rgb` deja de poder abortar el proceso** (`overlay.rs`): un color no
  ASCII en los ajustes rompía el troceado por bytes.

### Añadido
- **Persistencia atómica y a prueba de corrupción** (`jsonstore.rs`): todos los
  ficheros de datos se escriben en `<archivo>.tmp` y se renombran (una operación
  atómica), y al leerlos se distingue *no existe* de *corrupto*. Un JSON corrupto
  se **aparta** como `<archivo>.corrupt-<ts>` en vez de convertirse en valores por
  defecto que la siguiente escritura consolidaba: eso perdía silenciosamente
  favoritos, categorías o apps añadidas a mano.
- **Nombres de caché estables** (`art.rs`): las imágenes se nombraban con
  `DefaultHasher`, que Rust no garantiza estable entre versiones — al actualizar
  el toolchain se invalidaba toda la caché y los ficheros viejos quedaban ahí
  para siempre. Ahora es FNV-1a, con migración automática por renombrado (las
  portadas propias del usuario se migran leyendo `cover_overrides.json`).
- **Historial de sesiones acotado** a 500 por juego (`playtime.rs`), plegando el
  resto en el total; `playtime.json` se reescribe entero en cada sesión.
- **Medición de rendimiento** (`docs/perf/capture.ps1`, `perf.rs`): captura CPU,
  despertares, memoria, escrituras a disco y tamaño de cachés en reposo, y
  compara con una captura anterior. `METEOR_PERF=1` añade tiempos de
  `get_library`, `xbox::scan` y `art::resolve`.
- **Linter y pruebas de verdad**: `npm run lint` vuelve a existir (ESLint 9 con
  `react-hooks/exhaustive-deps` como error — la regla que detecta justo el fallo
  de memoización de la rejilla), Vitest para la búsqueda difusa y la paridad de
  catálogos es/en, y `npm run check` ejecuta todas las puertas de calidad de una
  vez.
- **Enlaces externos seguros** (`files.rs`): los enlaces de comunidad de la ficha
  abrían el navegador *dentro* de la propia ventana de la app; ahora pasan por un
  comando de Rust con lista blanca de dominios.
- **CI en cada push y pull request** (`.github/workflows/ci.yml`): typecheck,
  `next build`, `cargo clippy -D warnings` y `cargo test` en `windows-latest`.
  `release.yml` la reutiliza (`workflow_call`) y depende de ella, así que un tag
  ya no puede publicar algo que no pasaría CI. Todas las acciones fijadas por
  SHA. Nuevo `scripts/fetch-binaries.ps1`: deja `PresentMon.exe` (verificado) y
  `cputemp.exe` en `src-tauri/binaries/`, así un clon limpio puede compilar.
- **Primeras pruebas del repositorio** (`cargo test`): lista blanca de URIs y
  validación de rutas de `launcher.rs`, helpers de `files.rs` y regresión de
  `overlay::parse_rgb`.
- **Overlay adaptativo: detecta si cuesta FPS y reacciona (MPO)** (`metrics.rs`,
  `overlay_dcomp.rs`, `system.rs`, `OverlayMpoPanel.tsx`): un overlay sin inyección solo
  es **gratis** si Windows le concede un **plano hardware (MPO)**; en multimonitor o con
  refrescos mezclados, DWM lo **compone** y el juego pierde *independent-flip* → bajón de
  FPS/latencia. Ahora el overlay **se entera en vivo**: el sampler lee el modo de
  composición real del swapchain (`overlay::composition_mode`) tras una breve ventana de
  medición por sesión y clasifica el HUD como **libre** (plano hardware) o **costando**
  (DWM componiendo), con histéresis. Estado expuesto a la UI (`overlay_health` + evento
  `overlay-health`). Nuevo ajuste **`mpo_mode`**: `"always"` (mostrar siempre, como hasta
  ahora) o `"performance"` (si detecta que está costando, **se auto-oculta** para no bajar
  los FPS). Comando **`overlay_mpo_diagnostics`**: monitores activos, refresco por monitor
  + si están **mezclados**, y **HAGS** (programación de GPU por hardware) — los
  bloqueadores de MPO. La UI (pantalla in-game + Ajustes → Métricas) muestra un **badge de
  salud** (Sin coste / Costando FPS) y, cuando cuesta, los **pasos concretos** para que
  Windows conceda el plano hardware (igualar refrescos, activar HAGS, evitar multimonitor).
- **Diagnóstico profundo del overlay (opt-in)** (`overlay_diag.rs`): activable con la
  variable de entorno `METEOR_OVERLAY_DEBUG=1` (cero coste si no se pone — no hay spam
  por tick). Vuelca a **stderr y a `<app log dir>\overlay-debug.log`**: el backend
  elegido, la **decisión de gating** cada vez que cambia (overlay on, juego, pid, fg_pid,
  si dibuja) con la **ventana en primer plano clasificada** (borderless/exclusiva vs
  ventana, rect vs monitor — la condición que decide el MPO), un **heartbeat cada 3 s**
  con la muestra en vivo (fps/frame/gpu/cpu/temp/ram) y —lo clave— el **modo de
  composición real del swapchain** vía `IDXGISwapChainMedia::GetFrameStatisticsMedia`:
  `OVERLAY` (plano hardware/MPO → sin coste) vs `COMPOSED` (DWM compone → input lag) vs
  `COMPOSITION_FAILURE`. Es el test definitivo en runtime para saber si el overlay
  realmente está en un plano hardware o forzando composición.
- **Saludo personalizado en Inicio**: el «Bienvenido de vuelta» del Home ahora
  incluye el **nombre del usuario** de Windows. Comando Rust `username()`
  (`GetUserNameExW`/`NameDisplay`, con fallback a `%USERNAME%`); el front toma el
  primer nombre y lo capitaliza. Clave i18n `home.welcomeBackName`.
- **Tutorial guiado interactivo** (`GuidedTour.tsx`): un *product tour* de 12 pasos
  que resalta cada función **sobre la UI real** (recorte tipo coachmark + tooltip
  flotante), con navegación por teclado y anclaje mediante atributos `data-tour`.
  Enfoque mixto: **auto-dispara** el menú contextual y la ficha de detalle, e
  **ilustra** Spotlight, overlay, selección múltiple y arrastrar. Arranca tras el
  primer escaneo y es re-lanzable.
- **Botón «?»** en la barra superior (y en Ajustes → Aplicación) para **relanzar
  el tutorial** cuando se quiera.
- **Idiomas Español / Inglés** con **react-i18next**:
  - Infraestructura en `src/i18n/` (config, catálogos `en.ts`/`es.ts`,
    `I18nProvider`) con cambio de idioma **en vivo** en todas las ventanas.
  - Ajuste `AppSettings.language` (`system` | `es` | `en`; por defecto sigue el
    idioma del sistema y cae a inglés). **Selector** en Ajustes → Aplicación.
  - Traducido: Sidebar, TopBar, Footer, Home, GameCard, Spotlight, Splash,
    IntroSplash, UpdatePrompt, ConfirmDialog, Onboarding, NotificationsPanel,
    OverlaySettingsScreen, los menús/toasts/diálogos de `page.tsx`, **toda la pantalla
    de Ajustes** (Aplicación + Sistema + Métricas), la **ficha de detalle**
    (`DetailView`, namespace `detail`) y el **tutorial guiado** (`GuidedTour`, namespace
    `tour`; pasos con texto enriquecido vía `<Trans>` + componentes de icono/Kbd). Los
    géneros/modos de IGDB respetan el idioma. **Todos los diálogos** traducidos
    (namespace `dialog`): añadir app, carátula, categorías (individual/lote/nueva/
    editar) y elementos ocultos. Barrido del frontend **completo**.
  - **Sinopsis bilingüe**: `translate.rs` ahora traduce **por idioma de UI**
    (`translate(app, text, lang)`, `tl=<lang>`, caché por `lang:hash`; `en`
    devuelve el original de IGDB). `details_cache_v2.json` guarda la sinopsis
    **original en inglés** y `art::details(name, lang)` la traduce a la salida en
    cada petición; `game_details(name, lang)` recibe el idioma y `DetailView`
    re-pide al cambiarlo. La caché v1 (español horneado) se descarta. **i18n 100%.**

### Cambiado
- **Los juegos lanzados fuera de Meteor ya no se cronometran.** El watcher solo
  sigue a los que arrancan desde la app (era ya el comportamiento efectivo: el
  emparejamiento estaba limitado a esos), y a cambio en reposo no despierta ni
  enumera procesos.
- **Búsqueda: a igualdad de coincidencia gana el título más corto**
  (`fuzzy.ts`), así "portal" ordena *Portal* antes que *Portal Knights*.
- **Métricas más ligeras durante el juego (menos CPU en segundo plano)**: tres recortes
  quirúrgicos al subsistema de telemetría sin cambiar la arquitectura (sigue sin inyección de
  DLL, HUD nativo MPO-friendly, muestreo *gated*):
  - **Watcher de playtime `O(juegos activos)` en vez de `O(todos los procesos)`** (`playtime.rs`):
    enumeraba **todos** los procesos del sistema y resolvía el exe path de cada uno **cada 5 s**
    (un syscall por proceso, 300+ procesos), incluso con un juego ya identificado. Ahora guarda
    el **PID** de cada sesión activa y entre escaneos completos solo hace una **comprobación de
    vida barata por PID** (`proc_alive`, `OpenProcess`+`GetExitCodeProcess`, 1 syscall por juego);
    el escaneo completo se espacia (`FULL_SCAN_SECS`=20 s) o se fuerza si hay un lanzamiento
    pendiente. El PID publicado al overlay sale del mapa de sesiones (sin re-escanear). Coste
    aceptado: un juego lanzado **fuera** de Meteor puede tardar hasta ~20 s en detectarse (los
    lanzados desde Meteor siguen en ~5 s).
  - **Sampler sin `sysinfo` en el bucle por segundo** (`metrics.rs` + nuevo `sysstat.rs`): CPU% y
    RAM ahora vía Win32 directo (`GetSystemTimes` / `GlobalMemoryStatusEx`) en vez del refresco de
    `sysinfo::System` cada tick. Mismo dato mostrado (CPU% global + RAM usada/total), sin enumerar
    procesos ni asignar.
  - **Micro-opts ADLX (AMD)** (`adlx_shim.cpp` + `amd.rs`): se **cachea `TotalVRAM`** (constante por
    GPU) en vez de pedirlo en cada sample, y la **lectura de FPS** (`GetCurrentFPS`) se **omite**
    cuando el overlay no muestra ninguna métrica de FPS.
  - **Parseo de PresentMon sin asignación por frame** (`presentmon.rs`): a 200-800 FPS el parser
    hacía `split(',').collect::<Vec>()` por cada present; ahora extrae solo la columna de frametime
    con `split(',').nth(idx)` (sin `Vec`), recortando CPU en NVIDIA a FPS altos.
  - **Diagnóstico MPO multimonitor** (`overlay_diag.rs`): el reporte de la ventana en primer plano
    ahora incluye **nº de monitores** y si el juego está en el **monitor primario** (el HUD se pinta
    en el primario; un juego en monitor secundario explica que el HUD no se vea y que se pierda MPO).
- **Overlay sin WebView2 residente durante el juego (el cambio de rendimiento real)**: la
  ventana `overlay` (WebView2) se creaba **oculta al arrancar y vivía toda la sesión**. Una
  WebView2 oculta **mantiene viva toda su pila Chromium** (proceso browser + renderer +
  **proceso GPU**), que **compite con el juego en la composición de DWM** — justo lo que el
  HUD nativo se construyó para evitar, anulado por esta ventana fantasma. Ahora el HUD lo
  pinta solo la ventana nativa y la WebView del overlay **únicamente sirve la pantalla de
  ajustes**, así que se crea **bajo demanda** (`ensure_overlay_window`) al abrir los ajustes
  in-game y se **destruye al cerrarlos** (`set_overlay_interactive(false)` → `close()`).
  Resultado: **cero procesos Chromium del overlay mientras se juega** (antes ~3 procesos +
  RAM + proceso GPU residentes). El tamaño del monitor para el HUD se lee ahora de la ventana
  `main` (siempre presente) y la hotkey de ajustes la gestiona Rust (`toggle_overlay_settings`),
  ya que la ventana puede no existir para recibir un `emit`.
- **Overlay solo DirectComposition: eliminado el fallback GDI (decisión de rendimiento)**:
  tras testeo profundo, una ventana GDI `UpdateLayeredWindow` **nunca** es elegible para MPO,
  así que siempre saca al juego de *independent-flip* → input lag. Era el peor caso justo
  cuando más dolía. Ahora el HUD es **solo DComp**: si DComp no inicializa, el HUD se
  **desactiva** en vez de caer a GDI (no se entrega un overlay garantizado-laggy). Se
  borró toda la maquinaria GDI del HUD de `overlay_native.rs`, que queda reducido a los
  helpers de ventana en primer plano. Además: **restaurado el gate de foco** (un
  diagnóstico lo dejó dibujando sobre el escritorio con el juego en segundo plano) y
  **eliminado el log de depuración por tick** del sampler y de DComp. Coste residual del
  overlay = NVML+sysinfo a 1 s + un *present* por segundo en plano hardware cuando hay MPO
  ≈ cero; donde DWM deniega MPO, la composición es inevitable para cualquier ventana sobre
  el juego (límite de DWM, no del código).
- **HUD del overlay reescrito a una ventana nativa (mucho más ligero)**: el HUD ya
  **no es WebView2**. Un overlay de Chromium transparente corría su propio proceso
  GPU compitiendo con el juego y forzaba composición de DWM (input lag, sensación de
  bajo framerate). Ahora se pinta en una **ventana Win32 *layered* nativa**
  (`overlay_native.rs`): dibujo GDI a un **DIB ARGB de 32 bits** + `UpdateLayeredWindow`,
  *content-sized*, sin redirection bitmap → compone al mínimo y es *MPO-friendly*, así
  el juego conserva su ruta de baja latencia. El sampler (`metrics.rs`) dibuja el HUD
  directo en vez de emitir eventos a un webview. La **pantalla de ajustes in-game**
  sigue en el WebView2 `overlay` (visible solo al abrirla; el HUD nativo se oculta
  mientras tanto vía `set_settings_open`).
- **Overlay hiperligero: backend DirectComposition + flip swapchain (MPO real)**: el
  HUD GDI-`UpdateLayeredWindow` es ciudadano MPO de segunda (DWM lo compone por la ruta
  de *redirection*). Nuevo backend `overlay_dcomp.rs` que dibuja con **DirectComposition
  + DXGI flip swapchain + Direct2D/DirectWrite** — la superficie que DWM **promociona a
  plano hardware (MPO)**, así el juego conserva *independent-flip* y el overlay no añade
  composición ni input lag. Un facade `overlay.rs` elige DComp y, si el init D3D/DComp
  falla, **desactiva el HUD** (sin fallback GDI, ver más abajo). Mejoras transversales: **present-on-change** (no se
  redibuja si el texto no cambió), reposición de ventana solo si cambia, y constructor de
  filas **compartido** entre ambos backends (la lista de métricas no diverge). Sin crates
  nuevas: solo features del crate `windows` ya presente.
- **HUD solo cuando el juego está en foco**: el sampler ya solo dibuja (y muestrea) si la
  ventana del juego es la de **primer plano**. Al hacer alt-tab, el HUD se oculta y el
  sampler entra en reposo — antes componía un topmost sobre el escritorio para nada (el
  conteo de tiempo de juego sigue igual).
- **FPS en NVIDIA solo con admin (sin intentos en vacío)**: PresentMon (ETW) exige
  elevación, que no cambia en runtime, así que ahora se comprueba **una vez** al arrancar
  el controlador: sin admin **no se intenta lanzar nunca** (cero overhead, cero *access
  denied*). En AMD el FPS lo da ADLX sin admin igualmente. Resultado: hiperligero por
  defecto en ambos vendors.
- **Atajos globales por defecto**: ahora **F9** (Spotlight), **F10** (alternar
  overlay) y **F11** (ajustes del overlay), en lugar de combinaciones con Ctrl+Shift.
- La UI muestra siempre el **atajo real** (el personalizado del usuario o el
  por defecto) mediante un formateador común (`lib/shortcuts.ts`): Footer,
  tutorial, Ajustes, panel de notificaciones y ajustes del overlay.

### Eliminado
- **Ficha de metadatos de IGDB en el detalle** (sinopsis, vídeos/tráileres, galería
  promocional, géneros, temas, año, desarrollador/editor, nota, duración «time-to-beat»,
  webs oficiales y juegos similares): se quita por completo la llamada `game_details`
  (comando + `igdb::fetch_details` + `art::details` + cachés `details_cache*.json`) y el
  módulo de traducción de sinopsis (`translate.rs` + `translate_cache.json`). `DetailView`
  conserva **carátula, capturas propias** (Steam/Game Bar, no IGDB), **enlaces de
  comunidades** (PCGamingWiki, Nexus, ProtonDB, HowLongToBeat… construidos del nombre, sin
  red) y la **info local** (tamaño, tiempo jugado, sesiones). **Las carátulas de IGDB se
  mantienen** (`resolve_cover`), que es una llamada aparte. Resultado: al abrir un juego ya
  no se hace ninguna petición de ficha a IGDB ni a Google Translate.

### Seguridad y privacidad
- **La clave de firma del actualizador ya no comparte caché con nadie**
  (`release.yml`): el trabajo que la usa restauraba la caché de compilación de
  Rust, que escribe cualquier cambio subido a la rama principal. Eso significaba
  que contenido ajeno podía acabar enlazado dentro del binario firmado que se
  distribuye solo a todos los usuarios en cuestión de minutos. Ahora ese trabajo
  compila en frío.
- **Permisos mínimos en la automatización** (`ci.yml`, `release.yml`): ambos
  flujos declaran acceso de solo lectura por defecto y solo el paso de
  publicación obtiene escritura; además la credencial de la automatización deja
  de quedarse guardada en el repositorio descargado, donde la heredaban todos los
  pasos siguientes, incluidos los que instalan dependencias.
- **Dependencias con avisos de seguridad, actualizadas**: se cierran 5 avisos
  altos o críticos (Next.js, sharp, postcss, nanoid, browserslist) y se añade una
  comprobación de vulnerabilidades a la automatización para que los próximos no
  pasen desapercibidos.
- **El sidecar de temperatura ya no deja su driver de kernel cargado al cerrar
  Meteor** (`sidecar/cputemp/Program.cs`, `cputemp.rs`, `lib.rs`).
  LibreHardwareMonitor instala y arranca un driver de kernel para leer la
  temperatura del procesador, y solo se descarga llamando a `Close()`. El sidecar
  no tenía ningún camino de salida limpia: se le terminaba el proceso, lo que se
  salta los finalizadores de .NET, así que **el driver seguía cargado y
  registrado durante el resto del arranque de Windows** — y eso pasaba al cerrar
  la app normalmente, no solo al fallar. Los drivers de esa familia permiten
  lectura y escritura arbitraria de registros del procesador y de memoria física,
  están en la lista de drivers vulnerables bloqueados por Microsoft, y varios
  anti-cheat de kernel se niegan a arrancar el juego mientras uno esté cargado.
  Ahora Meteor le cierra la entrada estándar, el sidecar lo detecta, descarga el
  driver y sale; el Job Object queda solo como red de seguridad ante un cierre
  brusco.
- **La sesión ETW de PresentMon ya no queda huérfana** (`presentmon.rs`): una
  sesión de trazas en tiempo real es un objeto del kernel que **sobrevive al
  proceso que la creó**, así que terminar PresentMon la dejaba viva con sus
  búferes reservados hasta reiniciar, y Windows solo admite un número limitado a
  la vez. Ahora se para explícitamente por nombre. Además la sesión pasa a
  llamarse `Meteor-PresentMon`: antes se usaba el nombre por defecto y la opción
  de "parar la sesión existente" podía tumbar la de **otro programa** de captura
  de FPS que estuvieras usando.
- **Discord Rich Presence ya se puede desactivar, y viene desactivado**
  (`models.rs`, `discord.rs`, `SettingsDialog.tsx`): publicaba a qué jugabas a
  toda tu lista de amigos desde la primera ejecución, sin preguntar y **sin
  ninguna forma de apagarlo** — la tarjeta de ajustes estaba comentada en el
  código. Ahora es opcional y está en Ajustes → Aplicación, con el campo de
  Application ID propio dentro.
- **Los atajos globales ya no roban F9, F10 y F11 a todo el sistema**
  (`models.rs`, `lib.rs`): un atajo global se queda la combinación para toda la
  máquina, así que la ventana en primer plano deja de recibirla. Los valores por
  defecto eran teclas sueltas: F9 es guardado rápido en muchísimos juegos, F10
  abre el menú en cualquier aplicación de Windows y F11 es pantalla completa en
  todos los navegadores. Los nuevos valores son `Ctrl+Shift+F9`, `Ctrl+Shift+F10`
  y `Ctrl+Shift+F11`, y **si tenías los antiguos sin haberlos tocado, se migran
  solos**; si los personalizaste, se respetan. Además, cuando otra aplicación ya
  posee la combinación, el fallo se registra en vez de quedar en silencio.

### Corregido
- **Publicar una versión con la etiqueta equivocada ya no rompe las
  actualizaciones en silencio** (`release.yml`): nada comprobaba que la etiqueta
  coincidiera con la versión escrita en el proyecto. La publicación se nombra con
  la etiqueta pero el archivo que lee el actualizador toma la versión del
  proyecto, así que etiquetar `v0.2.0` sobre un árbol que dice `0.1.2` publicaba
  una versión llamada v0.2.0 que anunciaba 0.1.2 — y las actualizaciones se
  paraban sin ningún error visible. Ahora la publicación falla antes de subir
  nada.
- **Las carátulas descargadas de IGDB salían rotas** (`igdb.rs`, `art.rs`): la
  búsqueda devolvía una URL de imagen ya montada, pero quien la recibía la
  guardaba en el campo del **identificador** de imagen y volvía a montar una URL
  encima, así que el enlace final llevaba una dirección dentro de otra y no
  cargaba nunca — y además quedaba cacheado así. De paso se ignoraba el tamaño
  pedido y todo se resolvía siempre en la variante grande. Ahora la búsqueda
  devuelve el identificador y la URL se compone en un solo sitio, con una
  comprobación que rechaza cualquier cosa que no sea un identificador.
- **Un fallo de red dejaba la biblioteca sin carátulas durante tres días**
  (`igdb.rs`, `art.rs`): "IGDB no tiene este juego" y "no se ha podido preguntar
  a IGDB" acababan en el mismo resultado vacío, que se guardaba como respuesta
  válida y se respetaba durante tres días. Bastaba un escaneo sin conexión para
  que la biblioteca se quedara sin arte hasta que caducara. Ahora se distinguen y
  un fallo de conexión no escribe nada en la caché.
- **«Vaciar caché de portadas» no volvía a descargar nada** (`useLibrary.ts`): la
  lista de "ya resueltos" de la sesión solo crecía y nunca se limpiaba, así que
  tras vaciar la caché no quedaba ninguna entrada pendiente y no se pedía ni una
  carátula hasta reiniciar la aplicación. La única función cuyo propósito es
  volver a bajar el arte no hacía nada.
- **El cronómetro de juego y el HUD se enganchaban al proceso equivocado**
  (`playtime.rs`): la comprobación de "este proceso pertenece al juego" comparaba
  la ruta de instalación como prefijo de texto plano, sin frontera de separador,
  así que una carpeta `C:\Juegos\Foo` reclamaba también todo lo que corriera bajo
  `C:\Juegos\FooBar`. Con dos juegos de nombre parecido en la misma unidad, el
  tiempo jugado, el HUD de métricas y PresentMon se ataban al juego vecino. Ahora
  la ruta se compara sobre el separador y se tolera la barra final.
- **Un ejecutable auxiliar podía ganarle al ejecutable real del juego**
  (`playtime.rs`): las dos comprobaciones (ruta exacta del `.exe` y carpeta de
  instalación) vivían en la misma pasada sobre la lista de procesos, así que
  mandaba el **orden de enumeración** del sistema: un proceso cualquiera dentro
  de la carpeta que apareciera antes le ganaba al `.exe` exacto. Ahora el
  ejecutable conocido se busca primero en toda la lista.
- **Los servicios anti-cheat se podían confundir con el juego** (`playtime.rs`):
  las entradas de exclusión `easanticheat` y `battleye`/`be_service` no casaban
  con los procesos que realmente se instalan (`EasyAntiCheat.exe`,
  `BEService.exe`) y además solo se miraba el nombre del fichero, nunca la
  carpeta. Ahora se compara la ruta completa relativa a la instalación, así que
  `BattlEye\BEService.exe` y `EasyAntiCheat\EasyAntiCheat.exe` se descartan por
  su carpeta. Efecto práctico: el HUD y el contador dejan de seguir al proceso
  del anti-cheat en vez de al juego.
- **Juegos legítimos desaparecían de la biblioteca** (`windows_apps.rs`): el
  filtro de "entradas basura" del escaneo genérico del registro comparaba por
  subcadena, así que cualquier título que contuviera una palabra vetada se caía
  sin dejar rastro — *SteamWorld Dig 2* ("steam"), *Assassin's Creed Origins*
  ("origin"), *Driver: San Francisco* ("driver"). Las copias instaladas desde una
  tienda seguían apareciendo por su propio escáner, pero las instalaciones
  DRM-free o independientes se perdían. Ahora el filtro va por niveles: nombre
  completo para los clientes de tienda, fragmentos que solo existen en runtimes y
  drivers, y palabras genéricas únicamente en nombres de una o dos palabras.
  Puede que aparezca alguna entrada de sistema más que antes; ocultarla es un
  clic, y un juego que falta no se ve.
- **Overlay DirectComposition no iniciaba en GPUs AMD (HUD no aparecía)**: el swapchain
  de composición (`overlay_dcomp.rs`) se creaba con `Scaling: DXGI_SCALING_NONE`, que
  `CreateSwapChainForComposition` **rechaza** en muchos drivers (AMD incluido) con
  `DXGI_ERROR_INVALID_CALL` (`0x887A0001`); como el HUD in-game **no tiene fallback GDI**
  por diseño, fallaba el init y el overlay quedaba desactivado toda la sesión (las
  métricas se muestreaban bien, pero nada se dibujaba). Fix: usar `DXGI_SCALING_STRETCH`
  (el buffer va 1:1 con el contenido del HUD, así que no hay stretch real ni se pierde la
  elegibilidad MPO). De paso, `init()` ahora envuelve cada llamada DXGI/D3D/DComp en un
  paso etiquetado que registra **qué llamada concreta** falla en `overlay-debug.log`.
- **Panic en debug de sysinfo que tumbaba el watcher de playtime (y con él el overlay)**:
  sysinfo 0.30 hace `process_times/10_000_000 - 11_644_473_600` sin proteger la resta; si
  `GetProcessTimes` falla en un proceso protegido (queda 0), hay **underflow**. En release
  solo *wrap* (overflow-checks off → `start_time` basura que no leemos), pero en **debug
  hacía panic** y mataba el hilo del watcher — que es quien publica el juego al overlay, así
  que el HUD dejaba de aparecer. Fix: `[profile.dev.package.sysinfo] overflow-checks = false`
  (mismo comportamiento que release, solo para ese crate; nuestro código conserva los
  checks). Actualizar a 0.33.1 no servía: arrastra la misma resta.
- **Input lag del overlay en juegos *borderless* (clave)**: el HUD nativo llamaba a
  `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)` para no salir en grabaciones, pero
  la *display affinity* mete la ventana en la composición protegida de DWM y la
  **descalifica de MPO**. Sin MPO, una ventana *topmost* sobre un juego borderless saca
  a DWM de *independent-flip* y lo pasa a *composed-flip* → input lag y sensación de
  bajo framerate. Se elimina la llamada: el HUD vuelve a ser candidato a plano hardware
  (MPO) y el juego conserva su ruta de baja latencia. **Trade-off**: el HUD ahora sí
  aparece en capturas/grabaciones.
- **PresentMon dejaba de reintentar en bucle sin admin (golpea a NVIDIA)**: si
  PresentMon fallaba al lanzarse (ETW exige admin → *access denied*) o moría enseguida
  con el juego aún abierto, el controlador **reintentaba `CreateProcess` cada 500 ms**
  durante toda la partida = hitches. Afecta sobre todo a NVIDIA, donde PresentMon es la
  **única** fuente de FPS. Ahora recuerda el PID fallido (`failed_pid`) y no reintenta
  hasta que cambia el juego.
- **PresentMon ya no corre en GPUs AMD**: con AMD el FPS lo da **ADLX** nativo (sin
  admin), pero PresentMon se lanzaba igual (sesión ETW + parseo CSV por frame) para un
  dato que se descartaba. Ahora el sampler marca `ADLX_FPS_ACTIVE` cuando ADLX entrega
  FPS y la puerta `want_fps()` deja PresentMon **en reposo**; si se selecciona una GPU
  NVML o ADLX deja de dar FPS, PresentMon retoma.
- **Autostart no arrancaba con admin permanente**: si el instalador marcaba Meteor
  como administrador (flag `RUNASADMIN`) y se activaba el inicio con Windows, no
  arrancaba. Causa: el autostart usaba la clave `HKCU\...\Run`, que Windows
  **bloquea en silencio** en el login para apps que requieren UAC. Ahora, cuando
  Meteor corre elevado, el autostart se gestiona con una **tarea programada**
  (`MeteorAutostart`, `/SC ONLOGON /RL HIGHEST`) que sí arranca elevado sin prompt;
  la clave Run muerta se limpia y hay **migración automática** en el arranque para
  usuarios que ya la tenían (`elevation.rs` + `set_autostart`/`get_autostart`).
- **Input lag con el overlay de métricas activado**: la ventana del overlay cubría
  todo el monitor y rompía el *independent flip / MPO* del juego (en
  borderless/ventana), añadiendo 1–2 frames de latencia pese a tener FPS altos.
  Ahora el overlay es una **caja pequeña pegada a la esquina** y solo se agranda a
  pantalla completa al abrir sus ajustes.
- **Input lag y stutter del overlay (2ª pasada)**: dos causas más, resueltas.
  (1) La ventana usaba una caja **fija de 340×400** aunque el HUD ocupara mucho
  menos; ahora se **redimensiona al tamaño real del HUD**, minimizando la superficie
  compuesta y favoreciendo que DWM la **promueva a un plano hardware (MPO)**. (2) El
  overlay re-aseraba *topmost* (toggle `NOTOPMOST→TOPMOST`) **en cada tick**, forzando
  recomposición de DWM y *stutter periódico*; ahora solo re-asierta cuando **cambia la
  ventana en primer plano** (`metrics.rs`, gate por `last_fg`).

---

## [0.0.8]

### Añadido
- **Sistema de ajustes del launcher** con configuración del overlay y módulos de
  integración con tiendas.
- **Selector de GPU + panel «Mi equipo»** en Ajustes: el sampler inicializa NVML y
  ADLX a la vez, comando `system_info` (CPU/RAM/SO, discos, placa base, pantallas y
  lista de GPUs).
- Atajos globales **configurables** (Spotlight / alternar overlay / ajustes del
  overlay) editables desde Ajustes.

### Cambiado
- Mejora en la **detección de aplicaciones** (ampliación de `apps_db`) y
  **actualización dinámica de los atajos** en caliente.

---

## [0.0.7]

### Añadido
- **Monitorización de rendimiento**:
  - **FPS / frametime** vía **PresentMon** (ETW, sin inyección de DLL; requiere el
    binario y ejecutar como administrador).
  - **Temperatura de CPU** vía sidecar **LibreHardwareMonitor** (`cputemp.exe`,
    requiere admin y driver compatible con HVCI).
  - **GPU AMD** mediante **ADLX** (uso/temp/VRAM/clock/power y FPS), además de NVML
    para NVIDIA.
- **Elevación a administrador** bajo demanda (`is_elevated`, `restart_as_admin`)
  con aviso en la pestaña Métricas, y soporte de privilegios en el instalador.

---

## [0.0.6]

### Añadido
- **Home / Dashboard** como vista por defecto: tarjetas de estadísticas (tiempo
  total, esta semana, sesiones, juegos jugados), **«Continuar jugando»**, rankings
  de **juegos más jugados** y **apps más usadas**, todo derivado del tiempo de juego.

---

## [0.0.5]

### Añadido
- **IntroSplash**: pantalla de intro breve en cada arranque.
- **Página de detalle** enriquecida con metadatos de IGDB (sinopsis, géneros,
  modos, temas, perspectiva, saga, duración para completar, tráilers, galería,
  juegos similares) y enlaces dinámicos.
- **Capturas del usuario** en el detalle (Steam + Windows Game Bar).

---

## [Base inicial] — Fundamentos del launcher

### Biblioteca y fuentes
- **Biblioteca unificada** con escáneres nativos por tienda, cada uno en su módulo
  Rust y mezclados en `get_library` con deduplicación por nombre:
  **Steam, Epic, GOG, EA, Ubisoft, Xbox, Battle.net** (flavors de WoW), más fuentes
  curadas (Riot, Rockstar, Amazon) y el catch-all del registro de Windows.
- **Apps manuales** añadidas por el usuario.
- **`apps_db`**: gran librería de firmas para separar **aplicaciones** de **juegos**.
- **Icono real del .exe** para apps sin carátula (extraído de los recursos PE con
  `pelite`, cacheado como `.ico`).

### Carátulas y metadatos
- **Carátulas desde IGDB** (`art.rs` + `igdb.rs`) con caché en tres capas (imagen en
  disco → URL cacheada → consulta a la API) y servidas por el protocolo `asset`.
- **Override manual** de carátula por URL o **arrastrando una imagen local**.
- **Traducción de la sinopsis** al español (`translate.rs`, Google Translate `gtx`).

### Gestión y UX
- **Ocultar** elementos, **favoritos**, **categorías** (con iconos, drag & drop,
  reordenado, menú contextual) y **reclasificar juego ↔ aplicación**.
- **Búsqueda fuzzy**, **ordenación** (nombre / más jugados / recientes) y
  **selección múltiple** con acciones en lote.
- **Spotlight global**: paleta de lanzamiento rápida con atajo global, funciona
  aunque Meteor esté minimizado.
- **Menú contextual**, **arrastrar y soltar** a Favoritos/categorías, confirmación
  de acciones destructivas y **footer de atajos**.

### Sistema y plataforma
- **Tiempo de juego**: watcher global (`playtime.rs` + `sysinfo`) que cronometra
  cualquier juego se lance como se lance, con historial de sesiones y recuperación
  tras cierre.
- **Discord Rich Presence** (`discord.rs`, IPC local, client id embebido).
- **Bandeja del sistema** (cerrar oculta a la bandeja) e **iniciar con Windows**
  (`tauri-plugin-autostart`).
- **Auto-actualización** desde GitHub Releases (firmada, con barra de progreso y
  relaunch).
- **Onboarding** de primer arranque con splash de carga y auto-escaneo.

### Overlay de métricas in-game (fase inicial)
- HUD que se pinta **sobre el juego** con GPU/CPU/RAM (NVML/sysinfo), ventana
  transparente click-through, configurable (posición, métricas), con hotkey global.

---

## Tema y arquitectura

- **Tauri 2 (Rust) + Next.js 16** con export estático; toda la lógica de sistema en
  comandos Rust. UI con paleta rojo + azul, tipografía Oxanium/Source Code Pro,
  esquinas rectas y efectos de carta (tilt 3D + glow).

[No publicado]: #no-publicado--trabajo-en-curso
[0.0.8]: #008
[0.0.7]: #007
[0.0.6]: #006
[0.0.5]: #005
