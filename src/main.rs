mod capture;
mod color;
mod document;
#[cfg(target_os = "macos")]
mod macos;

fn main() {
    #[cfg(target_os = "macos")]
    macos::run();
    #[cfg(not(target_os = "macos"))]
    eprintln!("A interface do RXS está disponível apenas no macOS nesta versão.");
}
