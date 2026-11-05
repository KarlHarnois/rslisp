pub fn run_test(marker: &str, name: &str, summary: &str, location: &str, test: impl Fn()) {
    if !is_selected(name) {
        return;
    }
    println!("{marker}(test {} {})", Shown(name), Shown(summary));
    std::io::Write::flush(&mut std::io::stdout()).unwrap_or(());
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(test));
    match caught.err() {
        None => println!("{marker}(ok)"),
        Some(payload) => println!("{marker}({})", failure_record(&*payload, location)),
    }
}

fn failure_record(payload: &(dyn std::any::Any + Send), location: &str) -> String {
    let failed_check = FAILED_RECORD.lock().unwrap().take();
    failed_check
        .unwrap_or_else(|| format!("panic {} {}", Shown(location), Shown(panic_message(payload))))
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> &str {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("Box<dyn Any>")
}
