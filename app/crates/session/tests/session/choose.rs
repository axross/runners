//! what choosing a menu item does.

use marshal_session::{Act, Chosen, Confirmable, Then, choose};
use marshal_view::{Action, Confirmation, MenuItem};

fn question() -> Confirmation {
    Confirmation {
        title: "Stop all runners?".to_string(),
        body: "Any job running on them fails.".to_string(),
        confirm: "Stop".to_string(),
        cancel: "Cancel".to_string(),
    }
}

fn item(action: Action, available: bool, asks: bool) -> MenuItem {
    MenuItem {
        action,
        label: "An item".to_string(),
        available,
        reason: (!available).then(|| "Unavailable: because.".to_string()),
        confirmation: asks.then(question),
    }
}

#[test]
fn open_marshal_opens_the_window() {
    assert_eq!(
        choose(&item(Action::OpenMarshal, true, false), false),
        Chosen::Open
    );
}

#[test]
fn an_item_that_asks_nothing_runs_at_once() {
    assert_eq!(
        choose(&item(Action::StartAll, true, false), false),
        Chosen::Run(Act::StartAll)
    );
    assert_eq!(
        choose(&item(Action::BuildImage, true, false), true),
        Chosen::Run(Act::Build)
    );
}

#[test]
fn stop_all_asks_in_the_window_while_it_is_shown() {
    let chosen = choose(&item(Action::StopAll, true, true), true);
    assert_eq!(
        chosen,
        Chosen::AskInMainWindow {
            question: question(),
            then: Confirmable::StopAll,
        }
    );
}

#[test]
fn stopping_the_build_asks_in_the_window_while_it_is_shown() {
    let chosen = choose(&item(Action::StopBuild, true, true), true);
    assert_eq!(
        chosen,
        Chosen::AskInMainWindow {
            question: question(),
            then: Confirmable::StopBuild,
        }
    );
}

#[test]
fn stop_all_asks_in_a_window_of_its_own_when_the_main_window_is_hidden() {
    assert_eq!(
        choose(&item(Action::StopAll, true, true), false),
        Chosen::AskInOwnWindow {
            question: question(),
            then: Then::Act(Act::StopAll),
        }
    );
}

#[test]
fn stopping_the_build_asks_and_then_stops() {
    let Chosen::AskInOwnWindow { then, .. } = choose(&item(Action::StopBuild, true, true), false)
    else {
        panic!("a question in a window of its own");
    };
    assert_eq!(then, Then::Act(Act::StopBuild));
}

#[test]
fn an_act_the_page_has_no_question_for_asks_in_a_window_of_its_own_even_when_the_window_is_shown() {
    // no menu item asks before these today; one that did would not reach the page
    for (action, act) in [
        (Action::StartAll, Act::StartAll),
        (Action::BuildImage, Act::Build),
    ] {
        assert_eq!(
            choose(&item(action, true, true), true),
            Chosen::AskInOwnWindow {
                question: question(),
                then: Then::Act(act),
            }
        );
    }
}

#[test]
fn quit_always_asks_in_a_window_of_its_own() {
    for shown in [false, true] {
        assert_eq!(
            choose(&item(Action::Quit, true, true), shown),
            Chosen::AskInOwnWindow {
                question: question(),
                then: Then::Quit,
            },
            "shown: {shown}"
        );
    }
}

#[test]
fn quit_with_no_question_quits() {
    assert_eq!(
        choose(&item(Action::Quit, true, false), false),
        Chosen::Quit
    );
}

#[test]
fn an_unavailable_item_does_nothing() {
    for action in [
        Action::StopAll,
        Action::Quit,
        Action::OpenMarshal,
        Action::BuildImage,
    ] {
        assert_eq!(choose(&item(action, false, true), true), Chosen::Nothing);
    }
}

#[test]
fn a_token_action_is_not_the_menus() {
    let action = Action::RemoveToken("axross/runners".to_string());
    assert_eq!(choose(&item(action, true, true), true), Chosen::Nothing);
}

#[test]
fn try_again_tries_again() {
    assert_eq!(
        choose(&item(Action::TryAgain, true, false), false),
        Chosen::TryAgain
    );
}
