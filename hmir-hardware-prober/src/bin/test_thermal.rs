use sysinfo::Components;

fn main() {
    let components = Components::new_with_refreshed_list();
    println!("Found {} components", components.len());
    for component in components.list() {
        println!("{}: {}°C", component.label(), component.temperature());
    }
}
