# cputemp — sidecar de hardware

Pequeño ejecutable .NET que transmite al overlay de métricas la **temperatura de la
CPU** y la **telemetría de GPUs AMD** (uso, temperatura, consumo, reloj, VRAM y FPS).
Lo lanza y lo parsea `src-tauri/src/cputemp.rs`. El nombre viene de cuando solo leía
la CPU.

Usa [LibreHardwareMonitor](https://github.com/LibreHardwareMonitor/LibreHardwareMonitor)
(`LibreHardwareMonitorLib`, MPL-2.0).

## Modos

| Argumento     | Qué lee | Admin |
| ------------- | ------- | ----- |
| `--cpu`       | Temperatura de CPU (Ryzen Tctl/Tdie, núcleos Intel) | Sí, y PawnIO |
| `--gpu auto`  | La GPU AMD con más memoria (la dedicada antes que la integrada) | No |
| `--gpu <PnP>` | La GPU AMD cuyo id PnP contiene el fragmento, p. ej. `VEN_1002&DEV_7550&SUBSYS_88111EAE&REV_C0` | No |
| `--self-test` | Comprueba la elección de sensores sin abrir hardware (lo usa CI) | No |

Se pueden combinar `--cpu` y `--gpu`. Sin argumentos equivale a `--cpu`.

- **CPU:** la temperatura real solo se lee con un **driver de kernel**: PawnIO, que
  el usuario instala aparte desde pawnio.eu. La versión de LibreHardwareMonitor que
  usamos (`0.9.7-pre704`) ya no trae WinRing0 (Windows Defender lo marca como driver
  vulnerable) ni instala ningún driver. Sin admin o sin PawnIO, el sensor no aparece
  y la clave no se imprime; Astrail ni siquiera lanza el modo `--cpu`. No pasar a una
  versión estable de LHM hasta que alguna incluya PawnIO: la `0.9.4` aún usa WinRing0.
- **GPU:** se lee con ADL, la librería que instala el driver de AMD. El sidecar abre
  solo el grupo de GPUs AMD de LibreHardwareMonitor, no `Computer.Open()`, que
  además abriría el driver PawnIO cuando Astrail corre como admin. Las GPUs integradas
  no dan temperatura. `fps` solo aparece mientras una aplicación a pantalla completa
  exclusiva está presentando: ADL no cuenta los juegos en ventana sin bordes (probado
  con un juego sin bordes a 2560×1440 en una RX 9070 XT), y ahí el FPS lo da PresentMon.

## Protocolo

Una línea por segundo en stdout, con pares `clave=valor` separados por espacios
(cultura invariante). Una clave sin lectura no se imprime y la línea puede quedar
vacía:

```
cpu_temp=54 gpu_usage=9 gpu_temp=44 gpu_power=27.4 gpu_clock=152 vram_used=4340 vram_total=16304 fps=144
```

Unidades: °C, %, W, MHz, MB y fotogramas por segundo.

Para pararlo, el padre cierra el stdin. El sidecar ve EOF, cierra LibreHardwareMonitor
(libera los módulos de PawnIO con `--cpu`, para el registro de ADL con `--gpu`) y
sale. Matarlo con `TerminateProcess` se salta ese cierre. El servicio de PawnIO es de
su propio instalador y sigue instalado en cualquier caso.

## Compilar

```bash
dotnet publish src-tauri/sidecar/cputemp/cputemp.csproj -c Release -r win-x64 -o src-tauri/binaries
```

La forma de publicación (un solo `.exe` autocontenido, comprimido y recortado) está en
el `.csproj`, así que `scripts/fetch-binaries.ps1` y el workflow de release producen el
mismo binario. El `.exe` no se versiona.

`build.rs` incrusta el SHA-256 de `binaries/cputemp.exe` al compilar Astrail y
`cputemp.rs` se niega a lanzar un binario distinto. Tras recompilar el sidecar hay que
recompilar Astrail.

## Diagnóstico

Ejecutarlo a mano imprime una línea por segundo; se para con Ctrl+C.

- `--cpu` sin claves: no se pudo abrir PawnIO. Revisa que esté instalado y que el
  proceso esté elevado.
- `--gpu auto` sin claves: no hay GPU AMD o el driver de AMD no expone ADL. Con un
  fragmento PnP que no coincide, avisa por stderr.
