// Scaffolding for the component/extension runtime; not all of this surface is
// wired into the executor yet.
#![allow(dead_code)]

use super::types::ComponentId;

/// Store context for a WebAssembly component instance, holding WASI and HTTP state.
///
/// This is the wasmtime store data (`Store<Context>`); extensions naming
/// `Linker<Context>` to register host functions must be able to reference it,
/// so it stays `pub(crate)` even though the runtime owns its construction.
pub(crate) struct Context {
    owner: ComponentId,
    wasi_ctx: wasmtime_wasi::WasiCtx,
    wasi_http_ctx: wasmtime_wasi_http::WasiHttpCtx,
    http_hooks: CtxHttpHooks,
    resource_table: wasmtime_wasi::ResourceTable,
}

impl std::fmt::Debug for Context {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ctx").finish_non_exhaustive()
    }
}

impl wasmtime_wasi::WasiView for Context {
    fn ctx(&mut self) -> wasmtime_wasi::WasiCtxView<'_> {
        wasmtime_wasi::WasiCtxView {
            ctx: &mut self.wasi_ctx,
            table: &mut self.resource_table,
        }
    }
}

impl wasmtime_wasi_http::p2::WasiHttpView for Context {
    fn http(&mut self) -> wasmtime_wasi_http::p2::WasiHttpCtxView<'_> {
        wasmtime_wasi_http::p2::WasiHttpCtxView {
            ctx: &mut self.wasi_http_ctx,
            table: &mut self.resource_table,
            hooks: &mut self.http_hooks,
        }
    }
}

struct CtxHttpHooks;

impl wasmtime_wasi_http::p2::WasiHttpHooks for CtxHttpHooks {}

/// Builder for constructing a [`Context`] with WASI configuration.
///
/// Handed to extensions via [`super::extension::Extension::configure_ctx`], so it
/// is part of the extension-author surface; the runtime itself drives `new`/`build`.
pub(crate) struct ContextBuilder {
    owner: ComponentId,
    wasi_ctx_builder: wasmtime_wasi::WasiCtxBuilder,
}

impl ContextBuilder {
    /// Creates a new context builder with default settings.
    pub(super) fn new(owner: ComponentId) -> Self {
        Self {
            owner,
            wasi_ctx_builder: wasmtime_wasi::WasiCtx::builder(),
        }
    }

    /// Returns a reference to the owner component ID.
    pub(super) fn owner(&self) -> &ComponentId {
        &self.owner
    }

    /// Returns a mutable reference to the underlying WASI context builder.
    pub(crate) fn wasi_ctx_builder(&mut self) -> &mut wasmtime_wasi::WasiCtxBuilder {
        &mut self.wasi_ctx_builder
    }

    /// Builds the [`Context`] from the configured builder state.
    pub(super) fn build(mut self) -> Context {
        Context {
            owner: self.owner,
            wasi_ctx: self.wasi_ctx_builder.build(),
            wasi_http_ctx: wasmtime_wasi_http::WasiHttpCtx::new(),
            http_hooks: CtxHttpHooks,
            resource_table: wasmtime_wasi::ResourceTable::new(),
        }
    }
}

impl std::fmt::Debug for ContextBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ContextBuilder")
            .field("owner", &self.owner)
            .finish_non_exhaustive()
    }
}
