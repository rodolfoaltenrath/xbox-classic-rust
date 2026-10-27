//! Teste real do compilador: executa a CLI, compila o Rust emitido com Cargo/rustc
//! e roda verificações no binário gerado. Sem interpretador do próprio tradutor.

use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use xbox_formats::fixtures::xbe_com_codigo;
use xbox_recomp::{analysis, emit};

mod support;

fn check(command: &mut Command) -> String {
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "status={}\nstdout={}\nstderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn rust_emitido_compila_executa_e_restaura_a_continuacao() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let output = root.join(format!("target/recomp-tests/{}-{id}", std::process::id()));
    let cli = env!("CARGO_BIN_EXE_xberecomp");
    check(Command::new(cli).arg("demo").arg("--out").arg(&output));

    // Testa também a rota de tradução de um arquivo XBE já existente.
    let translated = output.join("translated");
    check(
        Command::new(cli)
            .arg("translate")
            .arg(output.join("src/image.xbe"))
            .arg("--out")
            .arg(&translated),
    );
    assert!(translated.join("src/generated.rs").is_file());

    // Uma segunda geração jamais sobrescreve silenciosamente o projeto anterior.
    assert!(
        !Command::new(cli)
            .arg("demo")
            .arg("--out")
            .arg(&output)
            .output()
            .unwrap()
            .status
            .success()
    );

    for (name, code) in [
        (
            "overflow",
            &[0xB8, 0xFF, 0xFF, 0xFF, 0xFF, 0x05, 1, 0, 0, 0, 0xC3][..],
        ),
        ("signed", &[0xB8, 0, 0, 0, 0, 0x83, 0xC0, 0xFF, 0xC3][..]),
        (
            "sib",
            &[
                0xBB, 0x20, 0x20, 1, 0, // mov ebx,0x12020
                0xB9, 1, 0, 0, 0, // mov ecx,1
                0xB8, 0x34, 0x12, 0, 0, // mov eax,0x1234
                0x89, 0x44, 0x8B, 0xFC, // mov [ebx+ecx*4-4],eax
                0x8B, 0x13, // mov edx,[ebx]
                0xC3,
            ][..],
        ),
        (
            "branch",
            &[
                0x31, 0xC0, 0x85, 0xC0, 0x74, 5, 0xB8, 99, 0, 0, 0, 0xB8, 7, 0, 0, 0, 0xC3,
            ][..],
        ),
        ("fault", &[0xA1, 0, 0, 0, 0x50, 0xC3][..]),
        (
            "write_code",
            &[0xB8, 0, 0, 0, 0, 0xA3, 0, 0x10, 1, 0, 0xC3][..],
        ),
        ("forever", &[0xEB, 0xFE][..]),
        ("ret_cleanup", &[0xC2, 8, 0][..]),
    ] {
        let image = xbe_com_codigo(code).unwrap();
        let program = analysis::analyze(&image).unwrap();
        std::fs::write(
            output.join(format!("src/{name}.rs")),
            emit::rust_source(&program),
        )
        .unwrap();
        std::fs::write(output.join(format!("src/{name}.xbe")), image).unwrap();
    }

    let mut main = std::fs::read_to_string(output.join("src/main.rs")).unwrap();
    main.push_str(include_str!("verification.inc"));
    support::extend_project(&output, &root, &mut main);
    check_cli_entries(cli, &output);
    std::fs::write(output.join("src/main.rs"), main).unwrap();
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let target = root.join("target/recomp-native-check");
    let tests = check(
        Command::new(&cargo)
            .arg("test")
            .arg("--offline")
            .arg("--manifest-path")
            .arg(output.join("Cargo.toml"))
            .arg("--target-dir")
            .arg(&target)
            .arg("--")
            .arg("--nocapture"),
    );
    let expected = native_test_summary();
    assert!(tests.contains(expected), "{tests}");
    std::fs::write(output.join("native-test-results.txt"), &tests).unwrap();
    let run = check(
        Command::new(&cargo)
            .arg("run")
            .arg("--offline")
            .arg("--manifest-path")
            .arg(output.join("Cargo.toml"))
            .arg("--target-dir")
            .arg(&target),
    );
    assert!(run.contains("EAX=62, instruções=37"), "{run}");
    check_generated_mains(cli, &output, &cargo, &target);
}

fn check_generated_mains(cli: &str, output: &Path, cargo: &std::ffi::OsStr, target: &Path) {
    check_stopped_main(cli, output, cargo, target);
    check_thread_main(cli, output, cargo, target);
    check_frame_export(cli, output, cargo, target);
    check_gamma_export(cli, output, cargo, target);
    check_state_file(cli, output, cargo, target);
}

fn check_gamma_export(cli: &str, output: &Path, cargo: &std::ffi::OsStr, target: &Path) {
    let project = output.join("gamma-export");
    check(
        Command::new(cli)
            .arg("translate")
            .arg(output.join("src/kernel_gamma.xbe"))
            .arg("--out")
            .arg(&project),
    );
    let raw = project.join("raw with spaces.bgra");
    let dac = project.join("dac with spaces.bgra");
    let run = |raw_path: &Path, dac_path: &Path| {
        Command::new(cargo)
            .args(["run", "--offline", "--manifest-path"])
            .arg(project.join("Cargo.toml"))
            .arg("--target-dir")
            .arg(target)
            .arg("--")
            .arg("--quadro")
            .arg(raw_path)
            .arg("--quadro-apresentado")
            .arg(dac_path)
            .output()
            .unwrap()
    };
    let first = run(&raw, &dac);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let raw_bytes = std::fs::read(&raw).unwrap();
    let dac_bytes = std::fs::read(&dac).unwrap();
    assert_eq!(raw_bytes.len(), 640 * 480 * 4);
    assert_eq!(dac_bytes.len(), raw_bytes.len());
    assert_eq!(&raw_bytes[..4], &[0xCC, 0x33, 0, 0xFF]);
    assert_eq!(&raw_bytes[2560..2564], &[0, 0x55, 0xAA, 0xFF]);
    assert_eq!(&dac_bytes[..4], &[0x99, 0x19, 0xFF, 0xFF]);
    assert_eq!(&dac_bytes[2560..2564], &[0x55, 0x2A, 0x55, 0xFF]);
    assert!(!run(&project.join("second-raw.bgra"), &dac).status.success());
    assert_eq!(std::fs::read(&dac).unwrap(), dac_bytes);
    assert!(!run(&raw, &project.join("second-dac.bgra")).status.success());
    assert_eq!(std::fs::read(&raw).unwrap(), raw_bytes);
}

fn check_state_file(cli: &str, output: &Path, cargo: &std::ffi::OsStr, target: &Path) {
    let project = output.join("state-main");
    check(
        Command::new(cli)
            .arg("translate")
            .arg(output.join("src/forever.xbe"))
            .arg("--out")
            .arg(&project),
    );
    let saved = project.join("estado com espaços.obsstate");
    let run = |args: &[&std::ffi::OsStr]| {
        Command::new(cargo)
            .args(["run", "--offline", "--manifest-path"])
            .arg(project.join("Cargo.toml"))
            .arg("--target-dir")
            .arg(target)
            .arg("--")
            .args(args)
            .output()
            .unwrap()
    };
    let first = run(&[
        "--fatias".as_ref(),
        "1".as_ref(),
        "--salvar".as_ref(),
        saved.as_os_str(),
    ]);
    assert!(!first.status.success());
    assert!(String::from_utf8_lossy(&first.stderr).contains("Estado salvo:"));
    let image = std::fs::read(project.join("src/image.xbe")).unwrap();
    let before = xbox_recomp::checkpoint::load_file(&saved, &image).unwrap();
    assert!(before.state.retired > 0);
    let next = project.join("segundo.obsstate");
    let second = run(&[
        "--fatias".as_ref(),
        "1".as_ref(),
        "--carregar".as_ref(),
        saved.as_os_str(),
        "--salvar".as_ref(),
        next.as_os_str(),
    ]);
    assert!(!second.status.success());
    assert!(String::from_utf8_lossy(&second.stderr).contains("Estado carregado:"));
    let after = xbox_recomp::checkpoint::load_file(&next, &image).unwrap();
    assert!(after.state.retired > before.state.retired);
    assert!(after.state.clock.cycles() > before.state.clock.cycles());
    let original = std::fs::read(&saved).unwrap();
    let third = run(&[
        "--fatias".as_ref(),
        "1".as_ref(),
        "--salvar".as_ref(),
        saved.as_os_str(),
    ]);
    assert!(String::from_utf8_lossy(&third.stderr).contains("Falha ao salvar estado"));
    assert_eq!(std::fs::read(&saved).unwrap(), original);
}

fn check_frame_export(cli: &str, output: &Path, cargo: &std::ffi::OsStr, target: &Path) {
    let project = output.join("frame-export");
    check(
        Command::new(cli)
            .arg("translate")
            .arg(output.join("src/kernel_display.xbe"))
            .arg("--out")
            .arg(&project),
    );
    let frame = project.join("frame with spaces.bgra");
    check(
        Command::new(cargo)
            .args(["run", "--offline", "--manifest-path"])
            .arg(project.join("Cargo.toml"))
            .arg("--target-dir")
            .arg(target)
            .arg("--")
            .arg("--quadro")
            .arg(&frame),
    );
    let bytes = std::fs::read(&frame).unwrap();
    assert_eq!(bytes.len(), 640 * 480 * 4);
    assert_eq!(&bytes[..4], &[0xCC, 0x33, 0, 0xFF]);
    assert_eq!(&bytes[2560..2564], &[0, 0x55, 0xAA, 0xFF]);
    let duplicate = Command::new(cargo)
        .args(["run", "--offline", "--manifest-path"])
        .arg(project.join("Cargo.toml"))
        .arg("--target-dir")
        .arg(target)
        .arg("--")
        .arg("--quadro")
        .arg(&frame)
        .output()
        .unwrap();
    assert!(!duplicate.status.success());
    assert_eq!(std::fs::read(&frame).unwrap(), bytes);
    let invalid = Command::new(cargo)
        .args(["run", "--offline", "--manifest-path"])
        .arg(project.join("Cargo.toml"))
        .arg("--target-dir")
        .arg(target)
        .args(["--", "--fatias", "1", "--typo"])
        .output()
        .unwrap();
    assert!(!invalid.status.success());
    assert!(!String::from_utf8_lossy(&invalid.stdout).contains("Execução nativa concluída"));
}

fn check_thread_main(cli: &str, output: &Path, cargo: &std::ffi::OsStr, target: &Path) {
    let project = output.join("thread-main");
    check(
        Command::new(cli)
            .arg("translate")
            .arg(output.join("src/thread_ret.xbe"))
            .arg("--out")
            .arg(&project)
            .args(["--entry", "0x11080", "--entry", "0x11100"]),
    );
    let run = check(
        Command::new(cargo)
            .args(["run", "--offline", "--manifest-path"])
            .arg(project.join("Cargo.toml"))
            .arg("--target-dir")
            .arg(target),
    );
    assert!(
        run.contains("EAX=0,") && run.contains("chamadas atendidas=2"),
        "{run}"
    );
}

fn check_stopped_main(cli: &str, output: &Path, cargo: &std::ffi::OsStr, target: &Path) {
    // Exercita também o main emitido: HLT e INT3 devem sair com diagnóstico, sem
    // anunciar conclusão nem girar até consumir o orçamento total.
    for (fixture, expected) in [
        ("control_assert", "HLT em 0x00011001"),
        ("bp_direct", "#BP (vetor 3) por INT3 em 0x00011000"),
    ] {
        let halted_project = output.join(format!("{fixture}-main"));
        check(
            Command::new(cli)
                .arg("translate")
                .arg(output.join(format!("src/{fixture}.xbe")))
                .arg("--out")
                .arg(&halted_project),
        );
        let halted = Command::new(cargo)
            .args(["run", "--offline", "--manifest-path"])
            .arg(halted_project.join("Cargo.toml"))
            .arg("--target-dir")
            .arg(target)
            .arg("--")
            .arg("--quadro")
            .arg(halted_project.join("no-frame.bgra"))
            .arg("--salvar")
            .arg(halted_project.join("missing-folder/state.obsstate"))
            .output()
            .unwrap();
        assert!(!halted.status.success());
        let diagnostic = String::from_utf8_lossy(&halted.stderr);
        assert!(diagnostic.contains(expected), "{diagnostic}");
        assert!(
            diagnostic.contains("Falha ao salvar estado"),
            "{diagnostic}"
        );
        let next = if fixture == "control_assert" {
            "próximo EIP=0x00011002, IF=0"
        } else {
            "próximo EIP=0x00011001"
        };
        assert!(diagnostic.contains(next), "{diagnostic}");
        assert!(!String::from_utf8_lossy(&halted.stdout).contains("Execução nativa concluída"));
    }
}

fn native_test_summary() -> &'static str {
    if cfg!(target_arch = "x86_64") {
        "273 passed"
    } else {
        "257 passed"
    }
}

fn check_cli_entries(cli: &str, output: &Path) {
    let image = output.join("src/ind_dynamic.xbe");
    let translated = output.join("cli-entries");
    check(
        Command::new(cli)
            .arg("translate")
            .arg(&image)
            .arg("--out")
            .arg(&translated)
            .args([
                "--entry", "0x11008", "--entry", "69646", "--entry", "0X11008",
            ]),
    );
    let bytes = std::fs::read(&image).unwrap();
    let expected =
        emit::rust_source(&analysis::analyze_with_entries(&bytes, &[0x11008, 0x1100E]).unwrap());
    assert_eq!(
        std::fs::read_to_string(translated.join("src/generated.rs")).unwrap(),
        expected
    );
    for address in ["0x100000000", "-1", "invalid", "0x11001", "0x12020"] {
        let invalid = output.join("cli-invalid-entry");
        let result = Command::new(cli)
            .arg("translate")
            .arg(&image)
            .arg("--out")
            .arg(&invalid)
            .args(["--entry", address])
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert!(!invalid.exists());
    }
}
