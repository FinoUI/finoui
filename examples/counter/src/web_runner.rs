//! wasm lifecycle, scenario, and probe runners for the shared counter.
//! 共享计数器的 wasm 生命周期、场景与探针运行器。

use std::{cell::RefCell, collections::BTreeMap, path::Path, rc::Rc};

use fino::{
    backend_web::{DomBackend, WebNode},
    dom_api::{Attribute, DomBackend as _, Tag},
    error::ToolError,
    headless::{Environment, HeadlessBackend as _, HeadlessHost},
    reactive::{Owner, Signal, batch},
    runtime::Runtime,
    scenario::{Scenario, ScenarioResult, run_on_host_collect},
};
use serde::Serialize;
use wasm_bindgen::prelude::*;

use crate::{
    runner::{mount_counter, mount_counter_with_state},
    show::{mount_show, run_show_conformance},
    web::new_host,
};

/// One mounted counter page with handles only to its own runtime.
/// 一个已挂载的计数器页面，只持有自己 Runtime 的句柄。
///
/// There is no global current page: the JavaScript caller owns this object.
/// 不存在全局当前页面：该对象由 JavaScript 调用方持有。
#[wasm_bindgen]
pub struct CounterPage {
    host: HeadlessHost<DomBackend>,
    count: Signal<i32>,
}

#[wasm_bindgen]
impl CounterPage {
    /// Mounts the shared counter on `document.body` exactly once.
    /// 只在 `document.body` 上挂载一次共享计数器。
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        let mut host = new_host(Environment::default());
        let slot: Rc<RefCell<Option<Signal<i32>>>> = Rc::new(RefCell::new(None));
        let capture = Rc::clone(&slot);
        host.mount("counter", move |runtime, owner| {
            let (root, count) = mount_counter_with_state(runtime, owner);
            runtime.backend_mut().mount_to_body(&root);
            *capture.borrow_mut() = Some(count);
            root
        });
        let count = slot.borrow_mut().take().expect("counter signal");
        Self { host, count }
    }

    /// Returns reactive and backend counters as a JSON string.
    /// 以 JSON 字符串返回响应式与后端计数。
    pub fn execution_counters(&self) -> String {
        serde_json::to_string(&self.host.execution_counters()).expect("counters JSON")
    }

    /// Applies one batched transaction with the requested number of increments.
    /// 在单个批处理事务内应用请求次数的增加。
    pub fn batch_increment(&self, times: u32) {
        batch(|| {
            for _ in 0..times {
                self.count.update(|number| *number += 1);
            }
        });
    }

    /// Disposes listeners, effects, and the mounted root.
    /// 释放监听、Effect 与已挂载根节点。
    pub fn dispose(&mut self) {
        self.host.dispose();
    }
}

impl Default for CounterPage {
    fn default() -> Self {
        Self::new()
    }
}

/// Runs the shared conformance sequence against the real browser DOM.
/// 针对真实浏览器 DOM 运行共享一致性序列。
#[wasm_bindgen]
pub fn run_conformance() -> Result<String, JsValue> {
    let host = new_host(Environment::default());
    let report = crate::conformance::run_conformance(host)
        .map_err(|message| tool_error("UI_CONFORMANCE", message))?;
    let violations = report.validate();
    let status = if violations.is_empty() {
        "passed"
    } else {
        "failed"
    };
    let mut value = serde_json::to_value(&report)
        .map_err(|error| tool_error("UI_CONFORMANCE_SERIALIZATION", error.to_string()))?;
    value["status"] = serde_json::Value::String(status.to_owned());
    value["violations"] = serde_json::to_value(&violations)
        .map_err(|error| tool_error("UI_CONFORMANCE_SERIALIZATION", error.to_string()))?;
    serde_json::to_string(&value)
        .map_err(|error| tool_error("UI_CONFORMANCE_SERIALIZATION", error.to_string()))
}

/// Runs the shared manual Show sequence against the real browser DOM.
/// 针对真实浏览器 DOM 运行共享手工 Show 序列。
#[wasm_bindgen]
pub fn run_show_conformance_probe() -> Result<String, JsValue> {
    let host = new_host(Environment::default());
    let report =
        run_show_conformance(host).map_err(|message| tool_error("UI_SHOW_CONFORMANCE", message))?;
    let violations = report.validate();
    let status = if violations.is_empty() {
        "passed"
    } else {
        "failed"
    };
    let mut value = serde_json::to_value(&report)
        .map_err(|error| tool_error("UI_CONFORMANCE_SERIALIZATION", error.to_string()))?;
    value["status"] = serde_json::Value::String(status.to_owned());
    value["violations"] = serde_json::to_value(&violations)
        .map_err(|error| tool_error("UI_CONFORMANCE_SERIALIZATION", error.to_string()))?;
    serde_json::to_string(&value)
        .map_err(|error| tool_error("UI_CONFORMANCE_SERIALIZATION", error.to_string()))
}

/// Whether an anchor is rejected as a text target while a real text node is accepted.
/// 锚点是否被拒绝作为文本目标，而真实文本节点被接受。
#[derive(Serialize)]
struct AnchorMisuseProbe {
    schema: u32,
    anchor_accepts_text: bool,
    text_accepts_text: bool,
}

/// Proves the Web backend keeps anchors invisible to `set_text`, matching Mock and Native.
/// 验证 Web 后端让锚点对 `set_text` 保持不可见，与 Mock 和 Native 一致。
#[wasm_bindgen]
pub fn run_anchor_misuse_probe() -> Result<String, JsValue> {
    let mut backend = DomBackend::new();
    let anchor = backend.create_anchor();
    let text = backend.create_text("text");
    serde_json::to_string(&AnchorMisuseProbe {
        schema: 1,
        anchor_accepts_text: backend.accepts_text(&anchor),
        text_accepts_text: backend.accepts_text(&text),
    })
    .map_err(|error| tool_error("UI_CONFORMANCE_SERIALIZATION", error.to_string()))
}

/// Observable state around remove and `mount_to_body`.
/// remove 与 `mount_to_body` 前后的可观测状态。
#[derive(Serialize)]
struct RemountProbe {
    schema: u32,
    mounted_matches: usize,
    removed_matches: usize,
    removed_tree_roots: usize,
    removed_a11y_nodes: usize,
    remounted_matches: usize,
    remounted_tree_roots: usize,
    remounted_a11y_nodes: usize,
}

/// Proves `mount_to_body` restores a removed node to the reachable tree.
/// 验证 `mount_to_body` 会把已移除节点恢复到可达树。
#[wasm_bindgen]
pub fn run_remount_probe() -> Result<String, JsValue> {
    let mut backend = DomBackend::new();
    let button = backend.create_element(Tag::Button);
    backend.set_attribute(&button, Attribute::AriaLabel, Some("Increment counter"));
    let text = backend.create_text("Count: 0");
    backend.insert_before(&button, &text, None);
    backend.mount_to_body(&button);
    let selector = crate::conformance::counter_selector();

    let mounted_matches = backend.find_all(&selector).len();
    backend.remove(&button);
    let removed_matches = backend.find_all(&selector).len();
    let removed_tree_roots = backend.logical_tree().roots.len();
    let removed_a11y_nodes = backend.a11y_snapshot().nodes.len();

    backend.mount_to_body(&button);
    let remounted_matches = backend.find_all(&selector).len();
    let remounted_tree_roots = backend.logical_tree().roots.len();
    let remounted_a11y_nodes = backend.a11y_snapshot().nodes.len();

    serde_json::to_string(&RemountProbe {
        schema: 1,
        mounted_matches,
        removed_matches,
        removed_tree_roots,
        removed_a11y_nodes,
        remounted_matches,
        remounted_tree_roots,
        remounted_a11y_nodes,
    })
    .map_err(|error| tool_error("UI_CONFORMANCE_SERIALIZATION", error.to_string()))
}

/// Runs the detach/reinsert probe against the real browser DOM.
/// 针对真实浏览器 DOM 运行分离／重插探针。
#[wasm_bindgen]
pub fn run_detach_reinsert_probe() -> Result<String, JsValue> {
    let host = new_host(Environment::default());
    let result = crate::conformance::run_detach_reinsert(host)
        .map_err(|message| tool_error("UI_CONFORMANCE", message))?;
    serde_json::to_string(&result)
        .map_err(|error| tool_error("UI_CONFORMANCE_SERIALIZATION", error.to_string()))
}

/// Artifacts returned to the browser driver for persistence.
/// 返回给浏览器驱动以便持久化的产物。
#[derive(Serialize)]
struct ScenarioBundle {
    result: ScenarioResult,
    artifacts: BTreeMap<String, String>,
}

/// Runs the unchanged JSON scenario against the real DOM inside the browser.
/// 在浏览器内针对真实 DOM 运行未修改的 JSON 场景。
#[wasm_bindgen]
pub fn run_scenario(scenario_json: &str, run_dir: &str) -> Result<String, JsValue> {
    let scenario: Scenario = serde_json::from_str(scenario_json)
        .map_err(|error| tool_error("UI_SCENARIO_INVALID_JSON", error.to_string()))?;
    let host = new_host(scenario.environment.clone());
    let run = run_on_host_collect(&scenario, Path::new(run_dir), host, mount_on_body)
        .map_err(tool_error_value)?;

    let artifacts = run
        .artifacts
        .into_iter()
        .map(|(path, bytes)| {
            String::from_utf8(bytes)
                .map(|contents| (path, contents))
                .map_err(|error| tool_error("UI_ARTIFACT_SERIALIZATION", error.to_string()))
        })
        .collect::<Result<BTreeMap<_, _>, JsValue>>()?;
    serde_json::to_string(&ScenarioBundle {
        result: run.result,
        artifacts,
    })
    .map_err(|error| tool_error("UI_ARTIFACT_SERIALIZATION", error.to_string()))
}

/// Runs the unchanged Show scenario against the real DOM inside the browser.
/// 在浏览器内针对真实 DOM 运行未修改的 Show 场景。
#[wasm_bindgen]
pub fn run_show_scenario(scenario_json: &str, run_dir: &str) -> Result<String, JsValue> {
    let scenario: Scenario = serde_json::from_str(scenario_json)
        .map_err(|error| tool_error("UI_SCENARIO_INVALID_JSON", error.to_string()))?;
    let host = new_host(scenario.environment.clone());
    let run = run_on_host_collect(&scenario, Path::new(run_dir), host, mount_show_on_body)
        .map_err(tool_error_value)?;

    let artifacts = run
        .artifacts
        .into_iter()
        .map(|(path, bytes)| {
            String::from_utf8(bytes)
                .map(|contents| (path, contents))
                .map_err(|error| tool_error("UI_ARTIFACT_SERIALIZATION", error.to_string()))
        })
        .collect::<Result<BTreeMap<_, _>, JsValue>>()?;
    serde_json::to_string(&ScenarioBundle {
        result: run.result,
        artifacts,
    })
    .map_err(|error| tool_error("UI_ARTIFACT_SERIALIZATION", error.to_string()))
}

/// Mounts the shared counter and attaches its root to `document.body`.
/// 挂载共享计数器并把其根节点附加到 `document.body`。
fn mount_on_body(runtime: &mut Runtime<DomBackend>, owner: &Owner) -> WebNode {
    let root = mount_counter(runtime, owner);
    runtime.backend_mut().mount_to_body(&root);
    root
}

/// Mounts the manual Show probe and attaches its root to `document.body`.
/// 挂载手工 Show 探针并把其根节点附加到 `document.body`。
fn mount_show_on_body(runtime: &mut Runtime<DomBackend>, owner: &Owner) -> WebNode {
    let root = mount_show(runtime, owner);
    runtime.backend_mut().mount_to_body(&root);
    root
}

/// Builds a serialized `ToolError` JavaScript exception for browser callers.
/// 为浏览器调用方构造序列化的 `ToolError` JavaScript 异常。
fn tool_error(code: &str, message: String) -> JsValue {
    let error = ToolError::for_backend(code, message, "web-headless");
    JsValue::from_str(&serde_json::to_string(&error).expect("tool error JSON"))
}

/// Converts an owned tool error into the same JavaScript exception shape.
/// 把已拥有的工具错误转换为相同的 JavaScript 异常形状。
fn tool_error_value(error: ToolError) -> JsValue {
    JsValue::from_str(&serde_json::to_string(&error).expect("tool error JSON"))
}
