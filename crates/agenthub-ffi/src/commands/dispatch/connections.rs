use super::*;

pub(super) async fn remote_get(
    ctx: DispatchContext<'_>,
    _args_json: &str,
) -> Result<String, String> {
    encode(remote::remoteGet(ctx.remote).await)
}

pub(super) async fn remote_ssh_hosts(
    _ctx: DispatchContext<'_>,
    _args_json: &str,
) -> Result<String, String> {
    encode(remote::remoteSshHosts().await)
}

pub(super) async fn remote_ssh_delete(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let args: WrappedRequest<SshDeleteRequest> = parse_args(args_json)?;
    encode(remote::remoteSshDelete(ctx.remote, args.request).await)
}

pub(super) async fn remote_verify(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let args: WrappedRequest<RemoteConnectionCredentials> = parse_args(args_json)?;
    encode(remote::remoteVerify(ctx.remote, args.request).await)
}

pub(super) async fn remote_save(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let args: WrappedRequest<RemoteConnectionCredentials> = parse_args(args_json)?;
    encode(remote::remoteSave(ctx.remote, args.request).await)
}

pub(super) async fn remote_install(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let args: WrappedRequest<RemoteConnectionCredentials> = parse_args(args_json)?;
    encode(remote::remoteInstall(ctx.remote, args.request).await)
}

pub(super) async fn remote_remove(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let args: WrappedRequest<RemoteRevisionRequest> = parse_args(args_json)?;
    encode(remote::remoteRemove(ctx.remote, args.request))
}

pub(super) async fn remote_select(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let args: WrappedRequest<RemoteSelectionRequest> = parse_args(args_json)?;
    encode(remote::remoteSelect(ctx.remote, args.request).await)
}

pub(super) async fn remote_invoke(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let args: WrappedRequest<RemoteInvokeRequest> = parse_args(args_json)?;
    let raw = remote::remoteInvokeRaw(ctx.remote, args.request).await?;
    Ok(String::from(Box::<str>::from(raw)))
}
