// This module is part of the perry-codegen-arkts crate. It was
// mechanically split out of the former monolithic lib.rs (issue
// #1100). Pure code move — no logic changes.
#![allow(clippy::too_many_arguments)]
use crate::*;

/// Issue #369 — emit four pieces of ArkTS glue for the perry/media
/// drain bridge: the imports, the per-instance Map of AVPlayer handles,
/// the drain methods, and the `aboutToAppear` lifecycle hook that kicks
/// off the 100 ms `setInterval` pump.
///
/// The pump dispatches one entry per drain per tick (matches the NAPI
/// shape — `drainMediaCreate` / `drainMediaControl` / `drainNowPlaying`
/// each pop a single intent). On a steady-state app the queues are
/// empty, so each tick is three NAPI roundtrips returning undefined —
/// cheap.
///
/// AVSession lock-screen integration is deferred — the now-playing
/// drain pulls the metadata but only calls `console.info(...)` on it
/// for now. Full AVSession plumbing requires the user's hap manifest
/// to declare `ohos.permission.AVSESSION` and is tracked as a follow-up
/// to #369.
pub(crate) fn media_glue() -> (String, String, String, String) {
    let imports = "\
         import media from '@ohos.multimedia.media';\n"
        .to_string();

    let decls = "\
    // perry/media — issue #369. Map<handle, AVPlayer> populated as
    // ArkTS drains createPlayer requests from the runtime queue.
    private mediaPlayers: Map<number, media.AVPlayer> = new Map();\n\
    private mediaPumpHandle: number = -1;\n"
        .to_string();

    // The `runMediaPump` method drives all three drains per tick. We
    // intentionally call each drain in a `while` loop so a multi-op
    // burst (e.g. user taps play+volume+seek in rapid succession) gets
    // coalesced into one tick of work rather than waiting one 100 ms
    // interval per op.
    let methods = "\
    aboutToAppear() {\n\
        this.mediaPumpHandle = setInterval(() => { this.runMediaPump(); }, 100);\n\
    }\n\
    \n\
    aboutToDisappear() {\n\
        if (this.mediaPumpHandle !== -1) { clearInterval(this.mediaPumpHandle); this.mediaPumpHandle = -1; }\n\
        this.mediaPlayers.forEach((p) => { try { p.release(); } catch (e) {} });\n\
        this.mediaPlayers.clear();\n\
    }\n\
    \n\
    runMediaPump() {\n\
        // 1) Allocate AVPlayers for each pending createPlayer.\n\
        let createReq: any = perryEntry.drainMediaCreate();\n\
        while (createReq !== undefined) {\n\
            this.allocPlayer(createReq.handle, createReq.url);\n\
            createReq = perryEntry.drainMediaCreate();\n\
        }\n\
        // 2) Dispatch every queued control op against its handle.\n\
        let cmd: any = perryEntry.drainMediaControl();\n\
        while (cmd !== undefined) {\n\
            this.dispatchControl(cmd);\n\
            cmd = perryEntry.drainMediaControl();\n\
        }\n\
        // 3) Now-playing metadata — best-effort. Wired to AVSession in\n\
        //    a follow-up; for now we just surface it in hilog so the\n\
        //    user can verify the bridge is alive.\n\
        let np: any = perryEntry.drainNowPlaying();\n\
        while (np !== undefined) {\n\
            console.info(`perry/media now-playing handle=${np.handle} title=${np.title} artist=${np.artist}`);\n\
            np = perryEntry.drainNowPlaying();\n\
        }\n\
    }\n\
    \n\
    allocPlayer(handle: number, url: string) {\n\
        media.createAVPlayer().then((player: media.AVPlayer) => {\n\
            this.mediaPlayers.set(handle, player);\n\
            player.on('stateChange', (state: string, _reason: any) => {\n\
                let cur: number = (player.currentTime !== undefined ? player.currentTime / 1000 : 0);\n\
                let dur: number = (player.duration !== undefined && player.duration > 0 ? player.duration / 1000 : 0);\n\
                perryEntry.pushMediaState(handle, state, cur, dur);\n\
                if (state === 'initialized') {\n\
                    player.prepare();\n\
                }\n\
            });\n\
            player.on('timeUpdate', (timeMs: number) => {\n\
                let cur: number = timeMs / 1000;\n\
                let dur: number = (player.duration !== undefined && player.duration > 0 ? player.duration / 1000 : 0);\n\
                perryEntry.pushMediaState(handle, 'playing', cur, dur);\n\
            });\n\
            player.on('error', (err: any) => {\n\
                console.error(`perry/media error handle=${handle} code=${err && err.code} msg=${err && err.message}`);\n\
                perryEntry.pushMediaState(handle, 'error', 0, 0);\n\
            });\n\
            player.on('endOfStream', () => {\n\
                perryEntry.pushMediaState(handle, 'completed', 0, 0);\n\
            });\n\
            player.url = url;\n\
        }).catch((err: any) => {\n\
            console.error(`perry/media createAVPlayer failed handle=${handle} url=${url} err=${err}`);\n\
            perryEntry.pushMediaState(handle, 'error', 0, 0);\n\
        });\n\
    }\n\
    \n\
    dispatchControl(cmd: any) {\n\
        const player: media.AVPlayer | undefined = this.mediaPlayers.get(cmd.handle);\n\
        if (player === undefined) { return; }\n\
        try {\n\
            switch (cmd.op) {\n\
                case 'play': player.play(); break;\n\
                case 'pause': player.pause(); break;\n\
                case 'stop': player.stop(); break;\n\
                case 'seek': player.seek(Math.floor(cmd.seconds * 1000)); break;\n\
                case 'setVolume': player.setVolume(cmd.volume); break;\n\
                case 'setRate':\n\
                    // AVPlayer.setSpeed takes an enum (0..6 mapped to 0.75x..2x).\n\
                    // Map the raw rate to the closest enum bucket.\n\
                    if (cmd.rate <= 0.5) { player.setSpeed(0); }\n\
                    else if (cmd.rate <= 0.875) { player.setSpeed(1); }\n\
                    else if (cmd.rate <= 1.125) { player.setSpeed(2); }\n\
                    else if (cmd.rate <= 1.375) { player.setSpeed(3); }\n\
                    else if (cmd.rate <= 1.75) { player.setSpeed(4); }\n\
                    else { player.setSpeed(5); }\n\
                    break;\n\
                case 'destroy':\n\
                    player.release();\n\
                    this.mediaPlayers.delete(cmd.handle);\n\
                    break;\n\
                default: break;\n\
            }\n\
        } catch (e) {\n\
            console.error(`perry/media dispatch failed op=${cmd.op} handle=${cmd.handle} err=${e}`);\n\
        }\n\
    }\n"
        .to_string();

    // Pump is started/stopped via the lifecycle methods above (declared
    // in `media_methods`), so there's nothing extra to add inside
    // `build()`. Returned slot stays as the empty string.
    let pump = String::new();

    (imports, decls, methods, pump)
}

/// Issue #369 — does this module use `perry/media`? Walks every statement
/// (init + every function body's statements) looking for any HIR
/// `Expr::NativeMethodCall { module: "perry/media", ... }`. Returns true
/// on first hit so the caller can opt the harvested .ets into the
/// `@ohos.multimedia.media` AVPlayer drain bridge.
pub(crate) fn module_uses_media(module: &Module) -> bool {
    fn stmts_use(stmts: &[Stmt]) -> bool {
        stmts.iter().any(stmt_uses)
    }
    fn stmt_uses(stmt: &Stmt) -> bool {
        match stmt {
            Stmt::Expr(e) | Stmt::Return(Some(e)) => expr_uses(e),
            Stmt::Let { init: Some(e), .. } => expr_uses(e),
            Stmt::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                expr_uses(condition)
                    || stmts_use(then_branch)
                    || else_branch.as_ref().map(|b| stmts_use(b)).unwrap_or(false)
            }
            Stmt::While {
                condition, body, ..
            }
            | Stmt::DoWhile {
                body, condition, ..
            } => expr_uses(condition) || stmts_use(body),
            Stmt::For {
                init,
                condition,
                update,
                body,
                ..
            } => {
                init.as_ref().map(|i| stmt_uses(i)).unwrap_or(false)
                    || condition.as_ref().map(expr_uses).unwrap_or(false)
                    || update.as_ref().map(expr_uses).unwrap_or(false)
                    || stmts_use(body)
            }
            _ => false,
        }
    }
    fn expr_uses(e: &Expr) -> bool {
        match e {
            Expr::NativeMethodCall {
                module: m, args, ..
            } => m == "perry/media" || args.iter().any(expr_uses),
            Expr::Call { callee, args, .. } => expr_uses(callee) || args.iter().any(expr_uses),
            Expr::Closure { body, .. } => stmts_use(body),
            Expr::Array(items) => items.iter().any(expr_uses),
            Expr::Object(fields) => fields.iter().any(|(_, v)| expr_uses(v)),
            _ => false,
        }
    }
    if stmts_use(&module.init) {
        return true;
    }
    for f in &module.functions {
        if stmts_use(&f.body) {
            return true;
        }
    }
    false
}
