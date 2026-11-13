//! Smoke window: proves Rune builds and renders on this machine. Plan 2 replaces the view.

use rune::{AppSpec, Cx, El, col, text};

#[derive(Default)]
struct State;

fn view(_: &State, _: &mut Cx<State>) -> El {
    let t = rune::tok::theme(true);
    col()
        .key("root")
        .w_full()
        .h_full()
        .center()
        .fill(t.bg)
        .child(text("MindRune").key("app-title"))
}

fn main() {
    rune::run(AppSpec::new(
        "MindRune",
        (1200.0, 520.0),
        State::default,
        view,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune::{Env, Runtime};

    #[test]
    fn the_window_shows_its_name() {
        let env = Env {
            system_dark: true,
            scale: 2.0,
        };
        let mut rt = Runtime::new(State, view, (1200.0, 520.0), env);
        rt.frame(0.0);
        assert!(rt.find("root").is_some());
        assert!(rt.find("app-title").is_some());
    }
}
