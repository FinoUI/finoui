//! wasm Host and Backend initialization for the shared counter.
//! 共享计数器的 wasm Host 与 Backend 初始化。

use fino::{
    backend_web::DomBackend,
    headless::{Environment, HeadlessHost},
};

/// Creates the browser host around the real DOM backend.
/// 围绕真实 DOM 后端创建浏览器 Host。
pub fn new_host(environment: Environment) -> HeadlessHost<DomBackend> {
    HeadlessHost::new(DomBackend::new(), environment)
}
