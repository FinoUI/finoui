//! Pins source spans and messages for the counter-stage `view!` diagnostics.
//! 固定计数器阶段 `view!` 诊断的源码位置与消息。

#[test]
fn view_macro_reports_unsupported_bindings() {
    let test_cases = trybuild::TestCases::new();
    test_cases.compile_fail("tests/view_macro_ui/ui/*.rs");
}
