pub fn note_untested(marker: &str, module: &str, exports: usize, untested: &str) {
    println!(
        "{marker}(untested {} {exports} {})",
        Shown(module),
        Shown(untested)
    );
}
