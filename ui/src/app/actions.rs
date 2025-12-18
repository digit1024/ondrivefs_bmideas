// SPDX-License-Identifier: MPL-2.0

use crate::app::context::ContextPage;

/// Application-level actions that are not page-specific
#[derive(Debug, Clone)]
pub enum ApplicationAction {
    ToggleContextPage(ContextPage),
}

