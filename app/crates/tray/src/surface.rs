//! what of the icon's menu and tooltip must be set again for a view: the menu when an
//! item's availability or label, or the summary, changed, and the tooltip when its words
//! did. Setting them again costs a redraw and closes an open menu, so nothing is set
//! that did not change.

use marshal_view::{Action, Menu};

/// the id of a menu item, which the menu event carries back; `None` for an action the
/// menu does not offer.
#[must_use]
pub fn menu_id(action: &Action) -> Option<&'static str> {
    match action {
        Action::OpenMarshal => Some("open_marshal"),
        Action::StartAll => Some("start_all"),
        Action::StopAll => Some("stop_all"),
        Action::BuildImage => Some("build_image"),
        Action::StopBuild => Some("stop_build"),
        Action::TryAgain => Some("try_again"),
        Action::Quit => Some("quit"),
        Action::SetToken(_) | Action::ReplaceToken(_) | Action::RemoveToken(_) => None,
    }
}

/// what makes the drawn menu differ: the summary, and each item's id, label and
/// availability.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Drawn {
    summary: String,
    items: Vec<(Option<&'static str>, String, bool)>,
}

impl Drawn {
    fn of(menu: &Menu) -> Self {
        Self {
            summary: menu.summary.clone(),
            items: menu
                .items
                .iter()
                .map(|item| (menu_id(&item.action), item.label.clone(), item.available))
                .collect(),
        }
    }
}

/// what to set on the icon.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Changes {
    pub menu: Option<Menu>,
    pub tooltip: Option<String>,
}

#[derive(Debug, Default)]
pub struct Surface {
    menu: Option<Drawn>,
    tooltip: Option<String>,
}

impl Surface {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// what differs from what was last shown; the first view differs in everything.
    pub fn show(&mut self, menu: &Menu, tooltip: &str) -> Changes {
        let drawn = Drawn::of(menu);
        let menu_changed = self.menu.as_ref() != Some(&drawn);
        if menu_changed {
            self.menu = Some(drawn);
        }
        let tooltip_changed = self.tooltip.as_deref() != Some(tooltip);
        if tooltip_changed {
            self.tooltip = Some(tooltip.to_string());
        }
        Changes {
            menu: menu_changed.then(|| menu.clone()),
            tooltip: tooltip_changed.then(|| tooltip.to_string()),
        }
    }
}
