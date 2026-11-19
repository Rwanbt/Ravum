//! STD-18.7 / CONC-2.3 under load: four producer threads send through clones
//! of one `Sender`; the consumer sums what it receives. The sum and the count
//! do not depend on interleaving, so 200 executions of one binary must print
//! the same two lines. A lost or duplicated message, a hang, or a crash fails.

use std::fs;
use std::process::Command;

const PROGRAM: &str = r#"fn produce(out: Sender<i64>) -> () effects(thread) {
    for i in 0..250 {
        let sent: bool = sync::send(&out, i as i64);
        drop(sent);
    }
}

fn main() -> () effects(io, thread, alloc) {
    let c: Channel<i64> = sync::channel();
    let tx: Sender<i64> = sync::sender(&c);
    let rx: Receiver<i64> = sync::receiver(c);
    let a: JoinHandle<()> = thread::spawn(produce, sync::clone_sender(&tx));
    let b: JoinHandle<()> = thread::spawn(produce, sync::clone_sender(&tx));
    let d: JoinHandle<()> = thread::spawn(produce, sync::clone_sender(&tx));
    let e: JoinHandle<()> = thread::spawn(produce, tx);
    let mut sum: i64 = 0;
    let mut count: i64 = 0;
    let mut done: bool = false;
    while !done {
        match sync::recv(&rx) {
            Some(v) => {
                sum = sum + v;
                count = count + 1;
            }
            None => {
                done = true;
            }
        }
    }
    drop(a);
    drop(b);
    drop(d);
    drop(e);
    io::println(sum.to_string().as_str());
    io::println(count.to_string().as_str());
}
"#;

#[test]
fn four_producers_give_the_same_total_on_every_run() {
    let root = std::env::temp_dir().join(format!("ravum-thread-stress-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("ravum.toml"),
        "[package]\nname = \"stress\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/main.rv"), PROGRAM).unwrap();
    let build = Command::new(env!("CARGO_BIN_EXE_ravum-lang"))
        .args(["build", "."])
        .current_dir(&root)
        .output()
        .expect("run ravum-lang build");
    assert!(
        build.status.success(),
        "build failed:\n{}{}",
        String::from_utf8_lossy(&build.stdout),
        String::from_utf8_lossy(&build.stderr)
    );
    let binary = root
        .join(".ravum/bin")
        .join(format!("stress{}", std::env::consts::EXE_SUFFIX));
    // 4 producers x (0 + 1 + ... + 249) = 4 x 31125.
    let expected = "124500\n1000\n";
    for run in 0..200 {
        let output = Command::new(&binary).output().expect("run the binary");
        assert!(output.status.success(), "run {run}: {:?}", output.status);
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
            expected,
            "run {run}"
        );
    }
    let _ = fs::remove_dir_all(&root);
}
