//! The order in which the launcher tries to get its own window drawn on Linux.
//!
//! A WebKitGTK window that stays white cannot be recognised from inside the
//! process: the window is there, the title is right, and nothing is ever
//! painted. Which combination of renderer settings draws depends on the
//! machine — the driver, the session, and, in an AppImage, on which libraries
//! the image brought along. Guessing one combination per release and asking
//! whoever owns the machine to try it takes a release cycle per guess, and
//! four of them have now missed on a Steam Deck.
//!
//! So the launcher works through the combinations itself. [`STEPS`] is that
//! ladder, from the setting that helps the most machines to the one that gives
//! up every piece of acceleration there is. The launcher starts on the first
//! step its session allows, and every time the interface fails to report for
//! duty it restarts itself on the next one. The step that finally draws is
//! remembered, so it costs the wait once per machine.
//!
//! This module is only the list and the arithmetic on it; applying a step and
//! restarting is the Tauri shell's job (`src-tauri/src/lib.rs`).

use serde::{Deserialize, Serialize};

/// What a step needs from the session to be worth trying at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Needs {
    /// Nothing — the step can always be tried.
    Nothing,
    /// An X server to point GTK and EGL at (`DISPLAY`).
    XServer,
    /// A Wayland compositor to talk to (`WAYLAND_DISPLAY`). No X server is
    /// asked for: a session that has none is exactly the one whose window
    /// cannot be an X11 window, so pointing GTK and EGL at Wayland is the
    /// only combination left to try there.
    WaylandSession,
}

/// One attempt at getting the window drawn: a name, and what it sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderStep {
    /// Stable identifier — written to the state file and to the log, so it
    /// must not change once a version carrying it has shipped.
    pub name: &'static str,
    /// Names and values to put into the environment before the window exists.
    /// Everything a step does not name is left at the value it had before the
    /// launcher ever touched it.
    pub env: &'static [(&'static str, &'static str)],
    /// The precondition for trying the step.
    pub needs: Needs,
    /// One line for the log and for a bug report.
    pub what: &'static str,
}

/// What the session offers. Read from the environment in the shell, passed in
/// here so the ladder can be exercised for sessions this machine is not in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Session {
    /// `DISPLAY` is set: an X server (or XWayland) to draw on.
    pub has_x_server: bool,
    /// `WAYLAND_DISPLAY` is set: the session is a Wayland one.
    pub is_wayland: bool,
}

impl Session {
    /// Read `DISPLAY` and `WAYLAND_DISPLAY` from this process.
    pub fn from_env() -> Self {
        Self {
            has_x_server: std::env::var_os("DISPLAY").is_some(),
            is_wayland: std::env::var_os("WAYLAND_DISPLAY").is_some(),
        }
    }

    fn allows(&self, step: &RenderStep) -> bool {
        match step.needs {
            Needs::Nothing => true,
            Needs::XServer => self.has_x_server,
            Needs::WaylandSession => self.is_wayland,
        }
    }
}

/// The ladder, in the order it is climbed.
///
/// `no-dmabuf` is first because it is what shipped as the unconditional
/// default and it is what the majority of the reports on a white GTK webview
/// are fixed by. `native` is second on purpose: WebKitGTK 2.42 and newer are
/// built around the DMA-BUF renderer, and where the launcher turns it off the
/// web process falls back to asking for the *default* EGL display — which is
/// precisely the call a Steam Deck reports as failing ("Could not create
/// default EGL display: EGL_BAD_PARAMETER. Aborting..."). A setting meant as
/// the fix is therefore a candidate for the cause, and until now no build let
/// anyone find out without knowing the escape hatch by name.
///
/// After that the two backends are pinned explicitly, because an AppImage
/// forces `GDK_BACKEND=x11` on every session whether or not that fits the
/// compositor, and Mesa picks its EGL platform from the environment rather
/// than from the window. Last come the steps that stop asking the GPU for
/// anything: this is a page of text and boxes, and llvmpipe draws it.
pub const STEPS: &[RenderStep] = &[
    RenderStep {
        name: "no-dmabuf",
        env: &[("WEBKIT_DISABLE_DMABUF_RENDERER", "1")],
        needs: Needs::Nothing,
        what: "WebKitGTK's DMA-BUF renderer off",
    },
    RenderStep {
        name: "native",
        env: &[],
        needs: Needs::Nothing,
        what: "nothing forced: WebKitGTK's own renderer, on the session's own backend",
    },
    RenderStep {
        name: "wayland",
        env: &[
            ("WEBKIT_DISABLE_DMABUF_RENDERER", "1"),
            ("GDK_BACKEND", "wayland"),
            ("EGL_PLATFORM", "wayland"),
        ],
        needs: Needs::WaylandSession,
        what: "window and EGL both on Wayland (undoes the AppImage's forced X11)",
    },
    RenderStep {
        name: "x11",
        env: &[
            ("WEBKIT_DISABLE_DMABUF_RENDERER", "1"),
            ("GDK_BACKEND", "x11"),
            ("EGL_PLATFORM", "x11"),
        ],
        needs: Needs::XServer,
        what: "window and EGL both on X11",
    },
    RenderStep {
        name: "software",
        env: &[
            ("WEBKIT_DISABLE_DMABUF_RENDERER", "1"),
            ("WEBKIT_DISABLE_COMPOSITING_MODE", "1"),
            ("LIBGL_ALWAYS_SOFTWARE", "1"),
            ("GALLIUM_DRIVER", "llvmpipe"),
            ("GDK_BACKEND", "x11"),
            ("EGL_PLATFORM", "x11"),
        ],
        needs: Needs::XServer,
        what: "no compositing, software GL on X11",
    },
    RenderStep {
        name: "software-surfaceless",
        env: &[
            ("WEBKIT_DISABLE_DMABUF_RENDERER", "1"),
            ("WEBKIT_DISABLE_COMPOSITING_MODE", "1"),
            ("LIBGL_ALWAYS_SOFTWARE", "1"),
            ("GALLIUM_DRIVER", "llvmpipe"),
            ("EGL_PLATFORM", "surfaceless"),
        ],
        needs: Needs::Nothing,
        what: "no compositing, software GL, and an EGL display that needs no display server",
    },
];

/// The first name this step sets that is already pinned to a different value
/// by whoever started the launcher, or `None` when the step is free to apply.
///
/// A step goes on whole or not at all. Half of `x11` is a window on X11 with
/// EGL left on Wayland — the very mismatch the ladder exists to undo — so a
/// step that cannot be applied completely is skipped and the climb moves on.
/// `pinned` answers what a name is pinned to, and `None` for a name the
/// launcher is free to set.
pub fn blocked_by(
    step: &RenderStep,
    pinned: &dyn Fn(&str) -> Option<String>,
) -> Option<&'static str> {
    step.env
        .iter()
        .find(|(name, value)| pinned(name).is_some_and(|set| set != *value))
        .map(|(name, _)| *name)
}

/// Every name any step sets. A restart puts all of them back to the value
/// they had before the launcher started, and only then applies the next step
/// — otherwise `native` would inherit the settings it is meant to do without.
pub fn touched_names() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = STEPS
        .iter()
        .flat_map(|s| s.env.iter().map(|(name, _)| *name))
        .collect();
    names.sort_unstable();
    names.dedup();
    names
}

/// The step of that name, whether or not the session allows it.
pub fn step(name: &str) -> Option<&'static RenderStep> {
    STEPS.iter().find(|s| s.name == name)
}

/// The first step to try in this session.
pub fn first(session: Session) -> &'static RenderStep {
    STEPS
        .iter()
        .find(|s| session.allows(s))
        // The last step needs nothing, so the find above cannot come up empty;
        // taking it as the fallback keeps that out of the callers all the same.
        .unwrap_or(&STEPS[STEPS.len() - 1])
}

/// The step after this one that the session allows, or `None` at the end of
/// the ladder. An unknown name (a state file from a newer build, or one
/// edited by hand) starts again from the top rather than ending the climb.
pub fn next_after(name: &str, session: Session) -> Option<&'static RenderStep> {
    let Some(at) = STEPS.iter().position(|s| s.name == name) else {
        return Some(first(session));
    };
    STEPS[at + 1..].iter().find(|s| session.allows(s))
}

/// The most conservative step the session allows — where `--safe-graphics`
/// jumps to.
pub fn safest(session: Session) -> &'static RenderStep {
    STEPS
        .iter()
        .rev()
        .find(|s| session.allows(s))
        .unwrap_or(&STEPS[STEPS.len() - 1])
}

/// Lines on which WebKitGTK has given up, rather than merely complained.
///
/// The web process says so and then stops; nothing is ever drawn, and no
/// amount of further waiting changes that. Recognising the line is what lets
/// the climb move to the next step in a second instead of sitting out the
/// watchdog's full patience in front of a window that is already dead.
///
/// Narrow on purpose. `libEGL warning: DRI3 error: Could not get DRI3 device`
/// appears on a machine that then renders perfectly well through software, so
/// "EGL" and "error" in a line mean nothing by themselves; only a line saying
/// the display could not be created at all, or WebKit's own parting word,
/// counts.
const GAVE_UP: &[&str] = &[
    // The Steam Deck's line, verbatim from its terminal:
    // "Could not create default EGL display: EGL_BAD_PARAMETER. Aborting..."
    "Could not create default EGL display",
    "Could not create EGL display",
    // What WebKit prints just before it takes the web process down.
    "Aborting...",
    // GTK could not reach the display server at all.
    "cannot open display",
    "Failed to initialize GTK",
];

/// Whether this line from the webview's standard error says it has given up.
pub fn looks_fatal(line: &str) -> bool {
    GAVE_UP.iter().any(|marker| line.contains(marker))
}

/// What the launcher remembers about this machine's graphics, between runs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GraphicsMemory {
    /// The step that drew last time. Used first on every later start.
    pub good: Option<String>,
    /// The step the current climb is on. Set before a restart, promoted to
    /// `good` once the interface reports for duty.
    pub trying: Option<String>,
    /// The step whose window the last run set out to build. Written before the
    /// window exists and cleared again the moment one does.
    ///
    /// A white window is watched for and climbed away from, but a *dead* one
    /// cannot be: a step whose settings take the process down before it has a
    /// window (`GDK_BACKEND=wayland` where `gtk_init` then fails, an EGL
    /// display the driver aborts over) leaves nothing running to notice. This
    /// field is what the next run reads instead — a step still named here got
    /// its turn and never came up, so it does not get another.
    pub attempted: Option<String>,
}

impl GraphicsMemory {
    /// Read the file's contents. Anything unreadable is "nothing remembered":
    /// a broken state file must not stop the launcher from starting.
    pub fn parse(text: &str) -> Self {
        serde_json::from_str(text).unwrap_or_default()
    }

    /// What to write back.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".into())
    }

    /// The step this memory says to start on: the climb in progress first, the
    /// step that worked next, and the top of the ladder when neither is known.
    /// A remembered name the session no longer allows (the machine moved from
    /// Wayland to X11) is dropped.
    pub fn step_to_use(&self, session: Session) -> &'static RenderStep {
        self.trying
            .as_deref()
            .or(self.good.as_deref())
            .and_then(step)
            .filter(|s| session.allows(s))
            .unwrap_or_else(|| first(session))
    }

    /// Whether this step took the last run down with it before it had a
    /// window, and so must not be tried again. A step that is also the one
    /// remembered as drawing is exempt: it has come up before, so the run
    /// that did not was something else going wrong.
    pub fn took_the_last_run_down(&self, name: &str) -> bool {
        self.attempted.as_deref() == Some(name) && self.good.as_deref() != Some(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOTH: Session = Session {
        has_x_server: true,
        is_wayland: true,
    };
    const X_ONLY: Session = Session {
        has_x_server: true,
        is_wayland: false,
    };
    const NEITHER: Session = Session {
        has_x_server: false,
        is_wayland: false,
    };

    #[test]
    fn the_ladder_has_unique_names_and_ends_somewhere_reachable() {
        let mut names: Vec<_> = STEPS.iter().map(|s| s.name).collect();
        let count = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), count, "step names must be unique");
        assert_eq!(
            STEPS.last().map(|s| s.needs),
            Some(Needs::Nothing),
            "the last step must be reachable in any session"
        );
    }

    #[test]
    fn the_climb_ends_after_finitely_many_steps() {
        for session in [BOTH, X_ONLY, NEITHER] {
            let mut seen = vec![first(session).name];
            while let Some(next) = next_after(seen[seen.len() - 1], session) {
                assert!(!seen.contains(&next.name), "the ladder loops: {seen:?}");
                seen.push(next.name);
                assert!(seen.len() <= STEPS.len(), "more steps than there are");
            }
            assert_eq!(seen[0], "no-dmabuf");
            assert_eq!(seen[seen.len() - 1], "software-surfaceless");
        }
    }

    #[test]
    fn a_session_without_a_compositor_skips_the_wayland_step() {
        let climbed: Vec<_> = {
            let mut out = vec![first(X_ONLY).name];
            while let Some(next) = next_after(out[out.len() - 1], X_ONLY) {
                out.push(next.name);
            }
            out
        };
        assert!(!climbed.contains(&"wayland"), "{climbed:?}");
        assert!(climbed.contains(&"x11"), "{climbed:?}");
    }

    #[test]
    fn a_session_without_an_x_server_skips_every_step_that_needs_one() {
        let mut climbed = vec![first(NEITHER).name];
        while let Some(next) = next_after(climbed[climbed.len() - 1], NEITHER) {
            climbed.push(next.name);
        }
        assert_eq!(climbed, vec!["no-dmabuf", "native", "software-surfaceless"]);
    }

    #[test]
    fn a_wayland_session_is_offered_the_wayland_step_with_or_without_xwayland() {
        for session in [
            BOTH,
            Session {
                has_x_server: false,
                is_wayland: true,
            },
        ] {
            let mut climbed = vec![first(session).name];
            while let Some(next) = next_after(climbed[climbed.len() - 1], session) {
                climbed.push(next.name);
            }
            assert!(climbed.contains(&"wayland"), "{climbed:?}");
        }
    }

    #[test]
    fn a_step_is_blocked_by_a_pin_that_disagrees_with_it_and_only_that() {
        let x11 = step("x11").expect("step present");
        assert_eq!(blocked_by(x11, &|_| None), None);
        // Pinned to what the step wants anyway: nothing to disagree with.
        assert_eq!(
            blocked_by(x11, &|n| (n == "EGL_PLATFORM").then(|| "x11".into())),
            None
        );
        assert_eq!(
            blocked_by(x11, &|n| (n == "EGL_PLATFORM").then(|| "wayland".into())),
            Some("EGL_PLATFORM")
        );
        // Whoever keeps the accelerated path keeps `native`, which sets
        // nothing and can therefore never be blocked.
        let pin = |n: &str| (n == "WEBKIT_DISABLE_DMABUF_RENDERER").then(|| "0".to_string());
        assert_eq!(
            blocked_by(step("native").expect("step present"), &pin),
            None
        );
        for name in [
            "no-dmabuf",
            "wayland",
            "x11",
            "software",
            "software-surfaceless",
        ] {
            assert_eq!(
                blocked_by(step(name).expect("step present"), &pin),
                Some("WEBKIT_DISABLE_DMABUF_RENDERER"),
                "{name} should stand aside for a pinned renderer"
            );
        }
    }

    #[test]
    fn safe_graphics_lands_on_the_bottom_of_the_ladder() {
        assert_eq!(safest(BOTH).name, "software-surfaceless");
        assert_eq!(safest(NEITHER).name, "software-surfaceless");
    }

    #[test]
    fn touched_names_cover_every_variable_any_step_sets() {
        let names = touched_names();
        for step in STEPS {
            for (name, _) in step.env {
                assert!(names.contains(name), "{name} missing from touched_names");
            }
        }
        // Sorted and free of repeats, so a restart resets each name once.
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(names, sorted);
    }

    #[test]
    fn native_forces_nothing_at_all() {
        assert!(step("native").expect("step present").env.is_empty());
    }

    #[test]
    fn an_unknown_remembered_step_starts_the_climb_again() {
        assert_eq!(
            next_after("from-a-newer-build", X_ONLY).map(|s| s.name),
            Some("no-dmabuf")
        );
        let memory = GraphicsMemory {
            good: Some("from-a-newer-build".into()),
            trying: None,
            attempted: None,
        };
        assert_eq!(memory.step_to_use(X_ONLY).name, "no-dmabuf");
    }

    #[test]
    fn a_climb_in_progress_outranks_the_step_that_worked_before() {
        let memory = GraphicsMemory {
            good: Some("no-dmabuf".into()),
            trying: Some("software".into()),
            attempted: None,
        };
        assert_eq!(memory.step_to_use(X_ONLY).name, "software");
    }

    #[test]
    fn a_remembered_step_the_session_no_longer_allows_is_dropped() {
        let memory = GraphicsMemory {
            good: Some("wayland".into()),
            trying: None,
            attempted: None,
        };
        assert_eq!(memory.step_to_use(X_ONLY).name, "no-dmabuf");
        assert_eq!(memory.step_to_use(BOTH).name, "wayland");
    }

    #[test]
    fn the_decks_own_line_reads_as_fatal_and_ordinary_grumbling_does_not() {
        assert!(looks_fatal(
            "Could not create default EGL display: EGL_BAD_PARAMETER. Aborting..."
        ));
        assert!(looks_fatal("Gdk-ERROR **: cannot open display: :0"));
        // Seen on a machine that went on to draw the interface without a
        // complaint from anyone: warnings are not failures.
        for line in [
            "libEGL warning: DRI3 error: Could not get DRI3 device",
            "libEGL warning: Ensure your X server supports DRI3 to get accelerated rendering",
            "** (nextgen-lan-launcher:19871): WARNING **: atk-bridge: get_device_events_reply:              unknown signature",
            "MESA-INTEL: warning: Performance support disabled, consider sysctl",
            "",
        ] {
            assert!(!looks_fatal(line), "{line:?} must not count as fatal");
        }
    }

    #[test]
    fn a_step_that_never_came_up_is_not_tried_again_unless_it_drew_before() {
        let memory = GraphicsMemory {
            good: None,
            trying: Some("wayland".into()),
            attempted: Some("wayland".into()),
        };
        assert!(memory.took_the_last_run_down("wayland"));
        assert!(!memory.took_the_last_run_down("x11"));
        // A step with a window to its name has come up before, so a run that
        // died is not the step's doing — one crash must not cost a machine
        // the setting it actually needs.
        let known_good = GraphicsMemory {
            good: Some("wayland".into()),
            trying: None,
            attempted: Some("wayland".into()),
        };
        assert!(!known_good.took_the_last_run_down("wayland"));
        // Cleared the moment a window exists, so an ordinary run says nothing.
        assert!(!GraphicsMemory::default().took_the_last_run_down("wayland"));
    }

    #[test]
    fn memory_survives_a_round_trip_and_a_broken_file() {
        let memory = GraphicsMemory {
            good: Some("x11".into()),
            trying: None,
            attempted: None,
        };
        assert_eq!(GraphicsMemory::parse(&memory.to_json()), memory);
        assert_eq!(GraphicsMemory::parse("not json"), GraphicsMemory::default());
        assert_eq!(GraphicsMemory::parse(""), GraphicsMemory::default());
        // A file from a build that knew other fields must still parse.
        assert_eq!(
            GraphicsMemory::parse(r#"{"good":"x11","tried_at":"yesterday"}"#)
                .good
                .as_deref(),
            Some("x11")
        );
    }
}
