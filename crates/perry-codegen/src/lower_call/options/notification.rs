//! perry/system `notificationSchedule({ id, title, body, trigger })`
//! options lowering (#96).
//!
//! Extracted from `lower_call.rs` (#1099, part of #1097) — pure move,
//! no behavior change. Switches on `trigger.type` (compile-time string
//! literal) to pick the interval/calendar/location runtime fn.

use anyhow::{bail, Result};
use perry_hir::Expr;

use super::extract_options_fields;
use crate::expr::{lower_expr, unbox_to_i64, FnCtx};
use crate::nanbox::double_literal;
use crate::types::{DOUBLE, I64, VOID};

/// Lower `notificationSchedule({ id, title, body, trigger })` (#96). Switches
/// on `trigger.type` (which must be a string literal at the call site so we
/// can pick the right runtime fn at compile time) and emits a flat-arg call
/// to one of three runtime fns:
/// - `interval` → `perry_system_notification_schedule_interval(id, title, body, seconds, repeats)`
/// - `calendar` → `perry_system_notification_schedule_calendar(id, title, body, timestamp_ms)`
/// - `location` → `perry_system_notification_schedule_location(id, title, body, lat, lon, radius)`
///
/// `repeats` is passed as a NaN-boxed JS value; the runtime calls
/// `js_is_truthy` to coerce. Missing fields default to 0.0.
pub(in crate::lower_call) fn lower_notification_schedule(
    ctx: &mut FnCtx<'_>,
    args: &[Expr],
) -> Result<String> {
    if args.len() != 1 {
        bail!(
            "notificationSchedule(...) takes one argument: \
             {{ id, title, body, trigger }} (got {} args)",
            args.len()
        );
    }
    let Some(props) = extract_options_fields(ctx, &args[0]) else {
        bail!(
            "notificationSchedule(...) requires an inline object literal: \
             {{ id: ..., title: ..., body: ..., trigger: {{ ... }} }}"
        );
    };

    let mut id_ptr: Option<String> = None;
    let mut title_ptr: Option<String> = None;
    let mut body_ptr: Option<String> = None;
    let mut trigger: Option<Vec<(String, Expr)>> = None;

    for (key, val) in &props {
        match key.as_str() {
            "id" => {
                let v = lower_expr(ctx, val)?;
                let blk = ctx.block();
                id_ptr = Some(unbox_to_i64(blk, &v));
            }
            "title" => {
                let v = lower_expr(ctx, val)?;
                let blk = ctx.block();
                title_ptr = Some(unbox_to_i64(blk, &v));
            }
            "body" => {
                let v = lower_expr(ctx, val)?;
                let blk = ctx.block();
                body_ptr = Some(unbox_to_i64(blk, &v));
            }
            "trigger" => {
                let Some(tprops) = extract_options_fields(ctx, val) else {
                    bail!(
                        "notificationSchedule: `trigger` must be an inline object literal \
                         like `{{ type: \"interval\", seconds: 60 }}`"
                    );
                };
                trigger = Some(tprops);
            }
            _ => {
                let _ = lower_expr(ctx, val)?;
            }
        }
    }

    let id_ptr = id_ptr
        .ok_or_else(|| anyhow::anyhow!("notificationSchedule: missing required field `id`"))?;
    let title_ptr = title_ptr
        .ok_or_else(|| anyhow::anyhow!("notificationSchedule: missing required field `title`"))?;
    let body_ptr = body_ptr
        .ok_or_else(|| anyhow::anyhow!("notificationSchedule: missing required field `body`"))?;
    let trigger = trigger
        .ok_or_else(|| anyhow::anyhow!("notificationSchedule: missing required field `trigger`"))?;

    let mut trigger_type: Option<String> = None;
    for (k, v) in &trigger {
        if k == "type" {
            match v {
                Expr::String(s) => trigger_type = Some(s.clone()),
                _ => bail!(
                    "notificationSchedule: `trigger.type` must be a string literal \
                     (one of \"interval\", \"calendar\", \"location\") at the call site"
                ),
            }
            break;
        }
    }
    let trigger_type = trigger_type.ok_or_else(|| {
        anyhow::anyhow!("notificationSchedule: missing required field `trigger.type`")
    })?;

    match trigger_type.as_str() {
        "interval" => {
            let mut seconds: String = "0.0".to_string();
            let mut repeats: String = double_literal(f64::from_bits(crate::nanbox::TAG_FALSE));
            for (k, v) in &trigger {
                match k.as_str() {
                    "type" => {}
                    "seconds" => seconds = lower_expr(ctx, v)?,
                    "repeats" => repeats = lower_expr(ctx, v)?,
                    _ => {
                        let _ = lower_expr(ctx, v)?;
                    }
                }
            }
            ctx.pending_declares.push((
                "perry_system_notification_schedule_interval".to_string(),
                VOID,
                vec![I64, I64, I64, DOUBLE, DOUBLE],
            ));
            ctx.block().call_void(
                "perry_system_notification_schedule_interval",
                &[
                    (I64, &id_ptr),
                    (I64, &title_ptr),
                    (I64, &body_ptr),
                    (DOUBLE, &seconds),
                    (DOUBLE, &repeats),
                ],
            );
        }
        "calendar" => {
            let mut timestamp_ms: String = "0.0".to_string();
            for (k, v) in &trigger {
                match k.as_str() {
                    "type" => {}
                    "date" => timestamp_ms = lower_expr(ctx, v)?,
                    _ => {
                        let _ = lower_expr(ctx, v)?;
                    }
                }
            }
            ctx.pending_declares.push((
                "perry_system_notification_schedule_calendar".to_string(),
                VOID,
                vec![I64, I64, I64, DOUBLE],
            ));
            ctx.block().call_void(
                "perry_system_notification_schedule_calendar",
                &[
                    (I64, &id_ptr),
                    (I64, &title_ptr),
                    (I64, &body_ptr),
                    (DOUBLE, &timestamp_ms),
                ],
            );
        }
        "location" => {
            let mut lat: String = "0.0".to_string();
            let mut lon: String = "0.0".to_string();
            let mut radius: String = "0.0".to_string();
            for (k, v) in &trigger {
                match k.as_str() {
                    "type" => {}
                    "latitude" => lat = lower_expr(ctx, v)?,
                    "longitude" => lon = lower_expr(ctx, v)?,
                    "radius" => radius = lower_expr(ctx, v)?,
                    _ => {
                        let _ = lower_expr(ctx, v)?;
                    }
                }
            }
            ctx.pending_declares.push((
                "perry_system_notification_schedule_location".to_string(),
                VOID,
                vec![I64, I64, I64, DOUBLE, DOUBLE, DOUBLE],
            ));
            ctx.block().call_void(
                "perry_system_notification_schedule_location",
                &[
                    (I64, &id_ptr),
                    (I64, &title_ptr),
                    (I64, &body_ptr),
                    (DOUBLE, &lat),
                    (DOUBLE, &lon),
                    (DOUBLE, &radius),
                ],
            );
        }
        other => bail!(
            "notificationSchedule: unknown trigger.type \"{}\" \
             (expected one of \"interval\", \"calendar\", \"location\")",
            other
        ),
    }

    Ok(double_literal(0.0))
}
