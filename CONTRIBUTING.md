# Contribuir a Astrail

Astrail es una app de escritorio **solo para Windows**: Tauri 2 (Rust) como núcleo
nativo y Next.js 16 (export estático) como interfaz. No hay servidor.

## Licencia de lo que aportas

Astrail es **GPL-3.0-only** con los términos adicionales de
[ADDITIONAL-TERMS.md](ADDITIONAL-TERMS.md). Al abrir un PR aceptas que tu código
se publique bajo esa misma licencia y esos mismos términos.

**No hay CLA**: conservas el copyright de lo que escribas. Quien decide qué entra
es el mantenedor ([GOVERNANCE.md](GOVERNANCE.md)).

Todo fichero de código escrito a mano empieza con esta cabecera, con el prefijo de
comentario que toque en cada lenguaje:

```
// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md
```

`node scripts/check-headers.mjs --fix` la añade donde falte, y `npm run check` la
verifica. La línea de copyright identifica la obra completa, no la autoría de cada
fichero: no la cambies al enviar un PR.

Si añades, quitas o actualizas una dependencia, regenera los avisos de terceros con
`node scripts/third-party-notices.mjs` y commitea el `THIRD-PARTY-NOTICES.txt`
resultante; la CI falla si está desactualizado.

## Preparar el entorno

Requisitos en el README. Una vez instalados:

```bash
npm install
powershell -File scripts/fetch-binaries.ps1   # PresentMon (verificado) + sidecar
npm run app                                   # tauri dev
```

`scripts/fetch-binaries.ps1` es obligatorio la primera vez: `tauri.conf.json`
declara `binaries/PresentMon.exe` y `binaries/cputemp.exe` como recursos, y sin
ellos ni siquiera `cargo check` compila. **No los sustituyas por ficheros vacíos.**

## Antes de abrir un PR

```bash
npm run check
```

Ejecuta ESLint, `tsc --noEmit`, Vitest, `cargo clippy -D warnings` y `cargo test`.
La CI (`.github/workflows/ci.yml`) corre lo mismo en `windows-latest`, y el
workflow de release depende de ella: un tag no puede publicar algo que no pase.

## Reglas que no son negociables

- **Nada de E/S en el hilo principal.** Todo comando que toque disco, red,
  registro o lance un proceso es `#[tauri::command(async)]`. El hilo principal es
  el mismo que el bucle de eventos, la bandeja, los atajos globales y el HUD.
- **Los comandos reciben ids, no structs.** Un `Game` que viene de la webview es
  entrada no confiable; se re-resuelve en Rust desde la caché de biblioteca.
- **Escrituras atómicas.** Todo lo que persista pasa por `jsonstore`
  (`<archivo>.tmp` + rename). Nunca un `fs::write` suelto a un fichero de datos.
- **Nada de `unwrap()`/`expect()` en hilos de fondo.** El perfil de release usa
  `panic = "abort"`: un panic se lleva la app entera.
- **Binarios del sistema por ruta absoluta** (`%SystemRoot%\System32\…`), nunca
  por `PATH`: el proceso puede estar elevado.
- **Todo texto visible pasa por i18n**, con la clave en `es.ts` **y** en `en.ts`.
  `npm run test` falla si los catálogos se desincronizan.
- **Cada corrección de bug lleva su prueba de regresión** (Rust: `#[cfg(test)]`
  junto al código; TS: `*.test.ts` junto al módulo).

## Rendimiento

Astrail vive en la bandeja del sistema y se dibuja encima de juegos: el coste en
reposo y el coste por fotograma son requisitos, no detalles.

- Mide antes y después: `powershell -File docs\perf\capture.ps1 -Label antes`
  (ver `docs/perf/README.md`). Un PR de rendimiento sin números no se puede
  revisar.
- Si tocas el HUD, arranca con `ASTRAIL_OVERLAY_DEBUG=1` y comprueba que el modo
  de composición sigue siendo `OVERLAY`. Si baja a `COMPOSED`, el overlay le está
  costando FPS al juego y el cambio no vale.
- Nada de inyección de DLL en procesos de juegos, por seguridad frente a
  anti-cheats. Esa decisión no se revisa.

## Estilo

- Comentarios y mensajes de commit en inglés; los textos de interfaz, el
  CHANGELOG y esta documentación en español.
- Commits: `feat: …`, `fix: …`, `refactor: …`, `perf: …`, `rel: …`. Una intención
  por commit.
- El CHANGELOG se actualiza en el mismo PR, bajo `[No publicado]`.
