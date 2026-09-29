// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

//! Every `#[tauri::command]`, grouped by concern. `lib.rs` keeps the builder
//! (`run`) and names each command by its module in the handler list. Every
//! module imports what it uses by name: the `use crate::*` each one had, and the
//! glob re-export of all six in `lib.rs`, hid which module depended on which
//! (2026-09-27 audit, K18).

pub(crate) mod backup;
pub(crate) mod library;
pub(crate) mod settings;
pub(crate) mod system;
pub(crate) mod update;
pub(crate) mod window;
