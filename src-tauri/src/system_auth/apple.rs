use super::VerifyOutcome;
use block2::RcBlock;
use objc2::runtime::Bool;
use objc2_foundation::{NSError, NSString};
use objc2_local_authentication::{LAContext, LAPolicy};
use std::sync::Mutex;
use tokio::sync::oneshot;

pub(super) fn map_error_code(code: isize) -> VerifyOutcome {
    match code {
        -2 | -4 | -9 => VerifyOutcome::Cancelled,
        -8..=-5 => VerifyOutcome::Unavailable,
        _ => VerifyOutcome::Failed,
    }
}

pub fn available() -> bool {
    unsafe {
        LAContext::new()
            .canEvaluatePolicy_error(LAPolicy::DeviceOwnerAuthentication)
            .is_ok()
    }
}

pub async fn verify(_app: &tauri::AppHandle, reason: &str) -> VerifyOutcome {
    let rx = start_evaluation(reason);
    rx.await.unwrap_or(VerifyOutcome::Failed)
}

// The block and context are not Send, so they must be gone before the await; the
// reply block holds the context so it is not released mid-evaluation.
fn start_evaluation(reason: &str) -> oneshot::Receiver<VerifyOutcome> {
    let (tx, rx) = oneshot::channel::<VerifyOutcome>();
    let context = unsafe { LAContext::new() };
    let pending = Mutex::new(Some((tx, context.clone())));
    let reply = RcBlock::new(move |ok: Bool, err: *mut NSError| {
        let outcome = if ok.as_bool() {
            VerifyOutcome::Ok
        } else {
            unsafe { err.as_ref() }.map_or(VerifyOutcome::Failed, |e| map_error_code(e.code()))
        };
        if let Some((tx, _context)) = pending.lock().unwrap().take() {
            let _ = tx.send(outcome);
        }
    });
    unsafe {
        context.evaluatePolicy_localizedReason_reply(
            LAPolicy::DeviceOwnerAuthentication,
            &NSString::from_str(reason),
            &reply,
        );
    }
    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_error_codes_map_to_outcomes() {
        assert_eq!(map_error_code(-2), VerifyOutcome::Cancelled);
        assert_eq!(map_error_code(-4), VerifyOutcome::Cancelled);
        assert_eq!(map_error_code(-9), VerifyOutcome::Cancelled);
        assert_eq!(map_error_code(-5), VerifyOutcome::Unavailable);
        assert_eq!(map_error_code(-6), VerifyOutcome::Unavailable);
        assert_eq!(map_error_code(-1), VerifyOutcome::Failed);
    }
}
