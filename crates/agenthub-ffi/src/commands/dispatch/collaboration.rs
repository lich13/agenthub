use super::*;

pub(super) async fn codex_desktop_attach(
    _ctx: DispatchContext<'_>,
    _args_json: &str,
) -> Result<String, String> {
    Err("codex.desktop.attach requires agenthub_ffi_desktop_attach stream entrypoint".into())
}

pub(super) async fn codex_desktop_action(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let args: WrappedRequest<DesktopActionRequest> = parse_args(args_json)?;
    encode(codex_desktop::action(ctx.state, ctx.remote, ctx.bridge, args.request).await)
}

pub(super) async fn codex_desktop_detach(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let args: WrappedRequest<DesktopSessionRequest> = parse_args(args_json)?;
    encode(codex_desktop::detach(ctx.bridge, args.request).await)
}

pub(super) async fn codex_desktop_open(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let args: WrappedRequest<DesktopThreadRequest> = parse_args(args_json)?;
    encode(codex_desktop::open(ctx.host, ctx.state, ctx.remote, args.request).await)
}

pub(super) async fn codex_desktop_catalog(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let args: WrappedRequest<DesktopCatalogRequest> = parse_args(args_json)?;
    encode(codex_desktop::catalog(ctx.state, ctx.remote, ctx.bridge, args.request).await)
}

pub(super) async fn codex_desktop_choose(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let args: WrappedRequest<DesktopChooseRequest> = parse_args(args_json)?;
    encode(codex_desktop::choose(ctx.host, ctx.remote, ctx.bridge, args.request).await)
}

pub(super) async fn codex_desktop_create(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let args: WrappedRequest<DesktopCreateRequest> = parse_args(args_json)?;
    encode(codex_desktop::create(ctx.host, ctx.state, ctx.remote, ctx.bridge, args.request).await)
}

pub(super) async fn codex_desktop_attachments(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let args: WrappedRequest<DesktopAttachmentRequest> = parse_args(args_json)?;
    encode(
        codex_desktop::attachments::handle(
            ctx.host,
            ctx.state,
            ctx.remote,
            ctx.bridge,
            args.request,
            None,
        )
        .await,
    )
}

pub(super) async fn codex_desktop_diagnostics(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let args: WrappedRequest<DesktopDiagnosticsRequest> = parse_args(args_json)?;
    encode(codex_desktop::diagnostics(ctx.host, ctx.remote, args.request).await)
}
