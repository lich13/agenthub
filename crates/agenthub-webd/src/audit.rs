pub(crate) struct AuditActor {
    pub(crate) admin_id: String,
}
pub(crate) fn actor() -> AuditActor {
    AuditActor {
        admin_id: "ssh-administrator".into(),
    }
}
