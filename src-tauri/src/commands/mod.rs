// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

//! Every `#[tauri::command]`, grouped by concern. `lib.rs` keeps the builder
//! (`run`), and re-exports these so the handler list and the tests name them
//! directly.

pub(crate) mod backup;
pub(crate) mod library;
pub(crate) mod settings;
pub(crate) mod system;
pub(crate) mod update;
pub(crate) mod window;
