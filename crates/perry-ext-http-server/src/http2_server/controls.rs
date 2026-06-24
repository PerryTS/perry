//! Session SETTINGS / PING / GOAWAY frame controls.

use super::*;

use perry_ffi::{get_handle, get_handle_mut, iter_handle_ids_of, JsValue};

use crate::types::{jsvalue_to_body_bytes, TAG_UNDEFINED};

pub(crate) fn numeric_value(value: f64) -> Option<f64> {
    let v = JsValue::from_bits(value.to_bits());
    if v.is_int32() || v.is_number() {
        Some(v.to_number())
    } else {
        None
    }
}

pub(crate) fn queue_session_ping(handle: i64, args: &[f64]) -> f64 {
    let first_callback = args
        .first()
        .copied()
        .map(|v| closure_arg(Some(v)))
        .unwrap_or(0);
    let second_callback = args
        .get(1)
        .copied()
        .map(|v| closure_arg(Some(v)))
        .unwrap_or(0);
    let (callback, payload_value) = if second_callback != 0 {
        (second_callback, args.first().copied())
    } else {
        (first_callback, None)
    };
    if callback == 0 {
        return bool_value(false);
    }
    let mut payload = payload_value
        .and_then(jsvalue_to_body_bytes)
        .unwrap_or_else(|| vec![0; 8]);
    if payload.len() != 8 {
        payload.resize(8, 0);
        payload.truncate(8);
    }
    if let Some(session) = get_handle_mut::<Http2SessionHandle>(handle) {
        session.pending_callbacks.push(callback);
    }
    push_h2_event(Http2PendingEvent::SessionPingCallback {
        session_handle: handle,
        callback,
        payload,
    });
    bool_value(true)
}

pub(crate) fn queue_session_settings(handle: i64, args: &[f64]) -> f64 {
    let settings_value_arg = args
        .first()
        .copied()
        .unwrap_or(f64::from_bits(TAG_UNDEFINED));
    let callback = args
        .get(1)
        .copied()
        .map(|v| closure_arg(Some(v)))
        .unwrap_or(0);
    let mut settings = get_handle::<Http2SessionHandle>(handle)
        .map(|session| session.local_settings.clone())
        .unwrap_or_default();
    settings.apply_value(settings_value_arg);
    if let Some(session) = get_handle_mut::<Http2SessionHandle>(handle) {
        session.local_settings = settings.clone();
        session.pending_settings_ack = true;
        if callback != 0 {
            session.pending_callbacks.push(callback);
        }
    }

    let caller_type = get_handle::<Http2SessionHandle>(handle)
        .map(|session| session.session_type)
        .unwrap_or(1);
    let peer_type = if caller_type == 1 { 0 } else { 1 };
    let local_server_handle = if caller_type == 1 {
        local_server_handle_for_client(handle)
    } else {
        None
    };
    let mut peer_ids = Vec::new();
    iter_handle_ids_of::<Http2SessionHandle, _>(|peer_id| {
        if get_handle::<Http2SessionHandle>(peer_id)
            .map(|session| {
                session.session_type == peer_type
                    && !session.closed
                    && !session.destroyed
                    && local_server_handle
                        .map(|server_handle| session.server_handle == server_handle)
                        .unwrap_or(true)
            })
            .unwrap_or(false)
        {
            peer_ids.push(peer_id);
        }
    });
    for peer_id in peer_ids {
        if let Some(session) = get_handle_mut::<Http2SessionHandle>(peer_id) {
            session.remote_settings = settings.clone();
            push_h2_event(Http2PendingEvent::SessionSettingsEvent {
                session_handle: peer_id,
                event: "remoteSettings",
                settings: settings.clone(),
            });
        }
    }
    if callback != 0 {
        push_h2_event(Http2PendingEvent::SessionSettingsCallback {
            session_handle: handle,
            callback,
            settings: settings.clone(),
        });
    }
    push_h2_event(Http2PendingEvent::SessionSettingsEvent {
        session_handle: handle,
        event: "localSettings",
        settings,
    });
    f64::from_bits(TAG_UNDEFINED)
}

pub(crate) fn queue_session_goaway(handle: i64, args: &[f64]) -> f64 {
    let code = args.first().and_then(|v| numeric_value(*v)).unwrap_or(0.0);
    let last_stream_id = args.get(1).and_then(|v| numeric_value(*v)).unwrap_or(0.0);
    let opaque_data = args
        .get(2)
        .copied()
        .and_then(jsvalue_to_body_bytes)
        .unwrap_or_default();
    let caller_type = get_handle::<Http2SessionHandle>(handle)
        .map(|session| session.session_type)
        .unwrap_or(1);
    let peer_type = if caller_type == 1 { 0 } else { 1 };
    let local_server_handle = if caller_type == 1 {
        local_server_handle_for_client(handle)
    } else {
        None
    };
    let mut peer_ids = Vec::new();
    iter_handle_ids_of::<Http2SessionHandle, _>(|peer_id| {
        if get_handle::<Http2SessionHandle>(peer_id)
            .map(|session| {
                session.session_type == peer_type
                    && !session.closed
                    && !session.destroyed
                    && local_server_handle
                        .map(|server_handle| session.server_handle == server_handle)
                        .unwrap_or(true)
            })
            .unwrap_or(false)
        {
            peer_ids.push(peer_id);
        }
    });
    for peer_id in peer_ids {
        push_h2_event(Http2PendingEvent::SessionGoaway {
            session_handle: peer_id,
            code,
            last_stream_id,
            opaque_data: opaque_data.clone(),
        });
    }
    f64::from_bits(TAG_UNDEFINED)
}
