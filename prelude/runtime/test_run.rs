pub fn start_run(marker: &str, names: Vec<&str>) {
    std::panic::set_hook(Box::new(|_| {}));
    let selected = names.iter().filter(|name| is_selected(name)).count();
    println!("{marker}(start {selected})");
}

pub fn finish_run(marker: &str) {
    println!("{marker}(done)");
}

fn is_selected(name: &str) -> bool {
    match std::env::args().skip(1).collect::<Vec<String>>().as_slice() {
        [flag, wanted] if flag == "--exact" => name == wanted,
        [filter] => name.contains(filter.as_str()),
        _ => true,
    }
}
