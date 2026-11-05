pub struct AuditActor {
    pub admin_id: String,
}
pub fn actor() -> AuditActor {
    AuditActor {
        admin_id: "ssh-administrator".into(),
    }
}
