use tokio::io::AsyncWriteExt;
pub async fn run(alias: &str, sudo: bool) -> Result<(), String> {
    let mut command = super::ssh::command(alias)?;
    command.arg(if sudo { "sudo -n bash -s" } else { "bash -s" });
    let mut child = command.spawn().map_err(|_| "无法启动 SSH 安装")?;
    let mut input = child.stdin.take().ok_or("无法写入安装脚本")?;
    let script = include_str!("../../../deploy/agenthub-webd/bootstrap.sh")
        .replace("@VERSION@", env!("CARGO_PKG_VERSION"))
        .replace(
            "@PUBLIC_KEY@",
            include_str!("../../../deploy/agenthub-webd/updater.pub").trim(),
        )
        .replace(
            "@VALIDATOR@",
            include_str!("../../../deploy/agenthub-webd/install-safety.py"),
        );
    input
        .write_all(script.as_bytes())
        .await
        .map_err(|_| "安装脚本发送失败")?;
    drop(input);
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(900),
        child.wait_with_output(),
    )
    .await
    .map_err(|_| "安装等待超时，请重新连接检查服务状态")?
    .map_err(|_| "远程安装失败")?;
    if !result.status.success() {
        let output = String::from_utf8_lossy(&result.stdout);
        let reason = output
            .lines()
            .find_map(|line| line.strip_prefix("AGENTHUB_INSTALL_ERROR:"));
        return Err(match reason {
 Some("system")=>"安装需要 Linux x86_64、systemd 和管理员权限",
 Some("dependencies")=>"系统依赖安装失败；Ubuntu/Debian 请检查软件源，其他发行版请先安装 curl、Python 3.11+ 和 Minisign",
 Some("download")=>"服务包下载失败，请检查远端网络后重试",
 Some("signature")=>"服务包签名或版本校验失败",
 Some("install")=>"服务安装失败，安装器已尝试恢复旧版",
 Some("health")=>"服务健康检查失败，请检查服务状态后重试",
 _=>"SSH 安装连接失败，请先在终端确认认证与免交互 sudo",
 }.into());
    }
    Ok(())
}
