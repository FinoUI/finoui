fn main() {
    let _ = fino::view! {
        <button>"Value: " {move || {
            let values = vec![0;];
            values[0]
        }}</button>
    };
}
