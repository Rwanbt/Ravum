use ui_core::motion::*;

const EASE: [f32; 4] = [0.42, 0.0, 0.58, 1.0];

fn trajectory(m: &mut Motion, key: u64, ms: u64) -> Vec<f32> {
    (0..ms)
        .map(|_| {
            m.advance(1);
            m.value(key).unwrap()
        })
        .collect()
}

#[test]
fn tweens_follow_their_curve_on_the_virtual_clock() {
    let mut m = Motion::default();
    m.set(1, 0.0);
    m.animate(1, Role::Transition, Curve::Linear { ms: 100 }, 100.0)
        .unwrap();
    m.advance(25);
    assert_eq!(m.value(1), Some(25.0));
    m.advance(75);
    assert_eq!(m.value(1), Some(100.0));
    assert_eq!(m.running(), 0);

    m.animate(1, Role::Transition, Curve::Bezier { ms: 300, p: EASE }, 0.0)
        .unwrap();
    let t = trajectory(&mut m, 1, 300);
    assert!(
        t.windows(2).all(|w| w[1] <= w[0]),
        "ease-in-out is monotonic"
    );
    assert!(
        (t[149] - 50.0).abs() < 1.0,
        "symmetric ease: half way at half time, got {}",
        t[149]
    );
    assert_eq!(*t.last().unwrap(), 0.0);
}

#[test]
fn springs_settle_and_respect_the_overshoot_bound() {
    let mut m = Motion::default();
    m.set(1, 0.0);
    // Sous-amorti modéré : dépasse un peu, dans la borne de Transition (10 %).
    let spring = Curve::Spring {
        stiffness: 0.001,
        damping: 0.05,
    };
    m.animate(1, Role::Transition, spring, 100.0).unwrap();
    let t = trajectory(&mut m, 1, 500);
    let peak = t.iter().cloned().fold(f32::MIN, f32::max);
    assert!(peak > 100.0 && peak <= 110.0, "peak {peak}");
    assert_eq!(*t.last().unwrap(), 100.0);
    assert_eq!(m.running(), 0);
}

#[test]
fn contracts_refuse_and_change_nothing() {
    let mut m = Motion::default();
    m.set(1, 0.0);
    let cases = [
        (
            Role::Feedback,
            Curve::Linear { ms: 400 },
            10.0,
            MotionError::TooLong { ms: 400, max: 150 },
        ),
        (
            Role::Feedback,
            Curve::Linear { ms: 100 },
            500.0,
            MotionError::TooLarge {
                amplitude: 500.0,
                max: 64.0,
            },
        ),
        (
            Role::Feedback,
            Curve::Linear { ms: 0 },
            10.0,
            MotionError::InvalidCurve,
        ),
        (
            Role::Transition,
            Curve::Bezier {
                ms: 100,
                p: [1.5, 0.0, 0.5, 1.0],
            },
            10.0,
            MotionError::InvalidCurve,
        ),
        (
            Role::Transition,
            Curve::Spring {
                stiffness: -1.0,
                damping: 0.1,
            },
            10.0,
            MotionError::InvalidCurve,
        ),
        (
            Role::Transition,
            Curve::Linear { ms: 100 },
            f32::NAN,
            MotionError::TooLarge {
                amplitude: f32::NAN,
                max: 2000.0,
            },
        ),
    ];
    for (role, curve, to, want) in cases {
        let got = m.animate(1, role, curve, to).unwrap_err();
        assert!(
            format!("{got:?}") == format!("{want:?}"),
            "{got:?} != {want:?}"
        );
        assert_eq!((m.value(1), m.running()), (Some(0.0), 0));
    }
    // Ressort trop souple : ne se stabilise pas dans le budget de Feedback.
    assert!(matches!(
        m.animate(
            1,
            Role::Feedback,
            Curve::Spring {
                stiffness: 0.0001,
                damping: 0.02
            },
            10.0
        ),
        Err(MotionError::TooLong { .. })
    ));
    // Ressort trop rebondissant pour Feedback (aucun dépassement permis).
    let e = m.animate(
        1,
        Role::Feedback,
        Curve::Spring {
            stiffness: 0.04,
            damping: 0.1,
        },
        10.0,
    );
    assert!(matches!(e, Err(MotionError::Overshoot { .. })), "{e:?}");
    assert!(matches!(
        m.animate(
            1,
            Role::Feedback,
            Curve::Spring {
                stiffness: 0.04,
                damping: 0.1
            },
            10.0
        ),
        Err(MotionError::Overshoot { .. })
    ));
    assert_eq!(m.running(), 0);
}

#[test]
fn retargeting_keeps_value_and_velocity_continuous() {
    let mut m = Motion::default();
    m.set(1, 0.0);
    let spring = Curve::Spring {
        stiffness: 0.001,
        damping: 0.07,
    };
    m.animate(1, Role::Transition, spring, 100.0).unwrap();
    m.advance(60);
    let (v, dv) = (m.value(1).unwrap(), m.velocity(1));
    assert!(dv > 0.0);
    m.animate(1, Role::Transition, spring, 0.0).unwrap();
    assert_eq!(m.value(1).unwrap(), v);
    assert!(
        (m.velocity(1) - dv).abs() < 1e-3,
        "{} vs {dv}",
        m.velocity(1)
    );
    // Elle continue d'abord dans le même sens, puis revient.
    m.advance(1);
    assert!(m.value(1).unwrap() > v);
    m.advance(500);
    assert_eq!(m.value(1), Some(0.0));
}

#[test]
fn interruption_budget_reduced_motion_and_offscreen() {
    let mut m = Motion::default();
    m.set(1, 0.0);
    m.animate(1, Role::Emphasis, Curve::Linear { ms: 600 }, 100.0)
        .unwrap();
    m.advance(100);
    assert_eq!(
        m.animate(1, Role::Transition, Curve::Linear { ms: 100 }, 0.0),
        Err(MotionError::NotInterruptible)
    );

    m.max_concurrent = 2;
    m.set(2, 0.0);
    m.set(3, 0.0);
    m.animate(2, Role::Transition, Curve::Linear { ms: 100 }, 10.0)
        .unwrap();
    assert_eq!(
        m.animate(3, Role::Transition, Curve::Linear { ms: 100 }, 10.0),
        Err(MotionError::Budget { max: 2 })
    );
    // Reciblage d'une animation en cours : ne compte pas comme une nouvelle.
    m.animate(2, Role::Transition, Curve::Linear { ms: 100 }, 20.0)
        .unwrap();

    // Hors écran : le temps de l'animation 1 n'avance plus.
    let before = m.value(1).unwrap();
    m.set_visible(1, false);
    m.advance(200);
    assert_eq!(m.value(1).unwrap(), before);
    m.set_visible(1, true);
    m.advance(100);
    assert!(m.value(1).unwrap() > before);

    let mut r = Motion::default();
    r.reduced_motion = true;
    r.set(1, 0.0);
    r.animate(1, Role::Transition, Curve::Linear { ms: 300 }, 80.0)
        .unwrap();
    assert_eq!((r.value(1), r.running()), (Some(80.0), 0));
    // La réduction ne contourne pas le contrat.
    assert!(r
        .animate(1, Role::Feedback, Curve::Linear { ms: 400 }, 0.0)
        .is_err());
}

#[test]
fn trajectories_are_deterministic() {
    let run = || {
        let mut m = Motion::default();
        m.set(1, 0.0);
        m.animate(
            1,
            Role::Transition,
            Curve::Spring {
                stiffness: 0.002,
                damping: 0.08,
            },
            300.0,
        )
        .unwrap();
        trajectory(&mut m, 1, 400)
    };
    let first = run();
    for _ in 0..10 {
        assert_eq!(run(), first);
    }
}
