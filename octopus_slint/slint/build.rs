fn main() {
    println!("cargo::rerun-if-changed=ui/app-window.slint");
    slint_build::compile("ui/app-window.slint").expect("Slint build failed");
}
