use agenthub_core::github_ci::Event;
use block2::{DynBlock, RcBlock};
use objc2::{
    define_class, msg_send,
    rc::Retained,
    runtime::{Bool, ProtocolObject},
    AnyThread, DefinedClass,
};
use objc2_foundation::{NSError, NSObject, NSObjectProtocol, NSString};
use objc2_user_notifications::*;
use std::cell::RefCell;
use tauri::Manager;
use tauri_plugin_opener::OpenerExt;

define_class!(
    // NSObject imposes no subclass requirements; the ivar is a thread-safe AppHandle.
    #[unsafe(super = NSObject)]
    #[name = "AgentHubCiNotificationDelegate"]
    #[ivars = tauri::AppHandle]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}
    unsafe impl UNUserNotificationCenterDelegate for Delegate {
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn present(
            &self,
            _center: &UNUserNotificationCenter,
            _notification: &UNNotification,
            completion: &DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
        ) {
            completion.call((UNNotificationPresentationOptions::Banner
                | UNNotificationPresentationOptions::List
                | UNNotificationPresentationOptions::Sound,));
        }

        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn clicked(
            &self,
            _center: &UNUserNotificationCenter,
            response: &UNNotificationResponse,
            completion: &DynBlock<dyn Fn()>,
        ) {
            // Both identifiers are supplied by this app or UserNotifications.
            if &*response.actionIdentifier() == unsafe { UNNotificationDefaultActionIdentifier } {
                let identifier = response.notification().request().identifier().to_string();
                if let Some((url, _)) = identifier.rsplit_once("#agenthub-ci:") {
                    if url.starts_with("https://github.com/") {
                        let _ = self.ivars().opener().open_url(url, None::<&str>);
                    }
                }
            }
            completion.call(());
        }
    }
);

thread_local! {
    // UNUserNotificationCenter keeps its delegate weakly. Retain it on the app thread.
    static DELEGATE: RefCell<Option<Retained<Delegate>>> = const { RefCell::new(None) };
}

pub(super) fn install(app: tauri::AppHandle) {
    let object = Delegate::alloc().set_ivars(app);
    // NSObject's designated initializer preserves the allocated class and ivars.
    let delegate: Retained<Delegate> = unsafe { msg_send![super(object), init] };
    UNUserNotificationCenter::currentNotificationCenter()
        .setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    DELEGATE.with(|slot| *slot.borrow_mut() = Some(delegate));
}

pub(super) fn send(app: &tauri::AppHandle, revision: u64, machine: &str, event: &Event) {
    let (title, body) = event.run.notification_text(machine);
    let identifier = format!(
        "{}#agenthub-ci:{}:{}",
        event.run.html_url,
        revision,
        event.key()
    );
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if !handle
            .state::<crate::remote::RemoteConnections>()
            .is_current(revision)
        {
            return;
        }
        let authorized = RcBlock::new(move |granted: Bool, _error: *mut NSError| {
            if !granted.as_bool()
                || !handle
                    .state::<crate::remote::RemoteConnections>()
                    .is_current(revision)
            {
                return;
            }
            let content = UNMutableNotificationContent::new();
            content.setTitle(&NSString::from_str(&title));
            content.setBody(&NSString::from_str(&body));
            content.setSound(Some(&UNNotificationSound::defaultSound()));
            let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
                &NSString::from_str(&identifier),
                &content,
                None,
            );
            let delivered = RcBlock::new(|error: *mut NSError| {
                if !error.is_null() {
                    eprintln!("CI system notification delivery failed");
                }
            });
            UNUserNotificationCenter::currentNotificationCenter()
                .addNotificationRequest_withCompletionHandler(&request, Some(&delivered));
        });
        UNUserNotificationCenter::currentNotificationCenter()
            .requestAuthorizationWithOptions_completionHandler(
                UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound,
                &authorized,
            );
    });
}
