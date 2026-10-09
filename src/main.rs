mod capture;
mod color;
mod document;
mod logs;
#[cfg(target_os = "macos")]
mod macos;
mod telemetry;

fn main() {
    let _telemetry = match telemetry::init() {
        Ok(guard) => Some(guard),
        Err(error) => {
            eprintln!("RSX: não foi possível iniciar os logs locais: {error}");
            None
        }
    };
    #[cfg(target_os = "macos")]
    macos::run();
    #[cfg(not(target_os = "macos"))]
    eprintln!("A interface do RSX está disponível apenas no macOS nesta versão.");
}
