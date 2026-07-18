package com.perry.app

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

/**
 * BroadcastReceiver behind Perry's scheduled local notifications (#96).
 *
 * `PerryBridge.scheduleInterval` / `scheduleCalendar` arm an AlarmManager
 * alarm whose PendingIntent targets this receiver with ACTION_FIRE; when the
 * alarm comes due (the process may have been dead — manifest receivers wake
 * it), the notification is built and posted here.
 *
 * Taps are NOT routed through this receiver: apps targeting API 31+ may not
 * start an activity from a receiver reached via a notification tap (the
 * "notification trampoline" block), so the tap PendingIntent goes directly
 * to PerryActivity, which calls `PerryBridge.handleNotificationTapIntent`.
 *
 * Registered in AndroidManifest.xml with `exported="false"` — the fire
 * intent is an app-internal PendingIntent.
 */
class PerryNotificationReceiver : BroadcastReceiver() {
    companion object {
        const val ACTION_FIRE = "com.perry.app.NOTIFICATION_FIRE"
        const val EXTRA_ID = "perry_notification_id"
        const val EXTRA_TITLE = "perry_notification_title"
        const val EXTRA_BODY = "perry_notification_body"
    }

    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != ACTION_FIRE) return
        val id = intent.getStringExtra(EXTRA_ID) ?: return
        val title = intent.getStringExtra(EXTRA_TITLE) ?: ""
        val body = intent.getStringExtra(EXTRA_BODY) ?: ""
        PerryBridge.postNotification(context, id, title, body)
    }
}
