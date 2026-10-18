fn main() -> std::process::ExitCode {
    let env = voice_enforcer::Env::from_process();
    let code = voice_enforcer::run(
        std::env::args_os(),
        &mut std::io::stdin().lock(),
        &mut std::io::stdout().lock(),
        &mut std::io::stderr().lock(),
        &env,
    );
    std::process::ExitCode::from(code)
}
