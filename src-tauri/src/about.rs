// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

//! What Settings → About shows.
//!
//! Section 5d of the GPL requires an interactive program to display its
//! Appropriate Legal Notices, and the additional terms name this screen as the
//! place where the author attribution lives. The three legal documents are
//! embedded with `include_str!` rather than bundled as resources, so they
//! travel with the executable and cannot go missing from an install.

use serde::Serialize;

/// The license itself, the section 7 terms it points to, and the notices for
/// the code Astrail ships from other people.
const LICENSE: &str = include_str!("../../LICENSE");
const ADDITIONAL_TERMS: &str = include_str!("../../ADDITIONAL-TERMS.md");
const THIRD_PARTY_NOTICES: &str = include_str!("../../THIRD-PARTY-NOTICES.txt");

/// The author, as the additional terms spell it. Every build must keep this.
pub const AUTHOR: &str = "Diego Alfonso Chicoma Ibañez (Dalfon.dev)";
/// The project the attribution has to link to.
pub const REPOSITORY: &str = "https://github.com/MrRobot4042212/Meteor";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AboutInfo {
    pub version: &'static str,
    pub author: &'static str,
    pub repository: &'static str,
    pub license: &'static str,
    /// Whether this build came out of the project's own release workflow.
    ///
    /// The release workflow sets `ASTRAIL_OFFICIAL_BUILD` only when it runs in
    /// the upstream repository, so a fork that builds the same tag gets `false`
    /// without having to remember to change anything — which is what term 2
    /// (GPL section 7c, mark modified versions) asks for.
    pub official: bool,
}

pub fn info() -> AboutInfo {
    AboutInfo {
        version: env!("CARGO_PKG_VERSION"),
        author: AUTHOR,
        repository: REPOSITORY,
        license: "GPL-3.0-only",
        official: option_env!("ASTRAIL_OFFICIAL_BUILD").is_some_and(|v| !v.is_empty()),
    }
}

/// One of the three legal documents, verbatim.
pub fn document(name: &str) -> Result<&'static str, String> {
    match name {
        "license" => Ok(LICENSE),
        "additional-terms" => Ok(ADDITIONAL_TERMS),
        "third-party-notices" => Ok(THIRD_PARTY_NOTICES),
        other => Err(format!("Unknown legal document: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn documents_are_the_real_ones() {
        assert!(LICENSE.contains("GNU GENERAL PUBLIC LICENSE"));
        assert!(LICENSE.contains("Version 3, 29 June 2007"));
        assert!(THIRD_PARTY_NOTICES.contains("Rust crates"));
        assert!(document("license").is_ok());
        assert!(document("nope").is_err());
    }

    /// The attribution the app shows and the one the terms require are the same
    /// strings, so editing one without the other fails here instead of shipping.
    #[test]
    fn attribution_matches_the_additional_terms() {
        assert!(ADDITIONAL_TERMS.contains(AUTHOR));
        assert!(ADDITIONAL_TERMS.contains(REPOSITORY));
        let about = info();
        assert_eq!(about.author, AUTHOR);
        assert_eq!(about.repository, REPOSITORY);
        assert!(crate::files::is_allowed_external(REPOSITORY));
    }
}
