fn main() {
    let _ = fino::view! {
        <button>"Value: " {move || {
            unknown_macro!();
            0
        }}</button>
    };
}
