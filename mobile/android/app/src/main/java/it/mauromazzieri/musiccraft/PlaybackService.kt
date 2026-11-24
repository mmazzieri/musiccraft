package it.mauromazzieri.musiccraft

import android.content.Intent
import androidx.media3.common.AudioAttributes
import androidx.media3.common.C
import androidx.media3.common.Player
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.session.MediaSession
import androidx.media3.session.MediaSessionService

/**
 * Plays in the background, with the system's media notification, lock-screen and headset
 * controls. Songs played to the end and songs skipped early are counted in the library.
 */
class PlaybackService : MediaSessionService() {
    private var session: MediaSession? = null

    override fun onCreate() {
        super.onCreate()
        MusicLibrary.start(this)
        val music = AudioAttributes.Builder()
            .setUsage(C.USAGE_MEDIA)
            .setContentType(C.AUDIO_CONTENT_TYPE_MUSIC)
            .build()
        val player = ExoPlayer.Builder(this)
            .setAudioAttributes(music, true)
            .setHandleAudioBecomingNoisy(true)
            .build()
        player.addListener(Counting(player))
        session = MediaSession.Builder(this, player).build()
    }

    override fun onGetSession(controllerInfo: MediaSession.ControllerInfo): MediaSession? = session

    override fun onTaskRemoved(rootIntent: Intent?) {
        val player = session?.player
        if (player == null || !player.playWhenReady || player.mediaItemCount == 0) stopSelf()
    }

    override fun onDestroy() {
        session?.run {
            player.release()
            release()
        }
        session = null
        super.onDestroy()
    }

    private class Counting(private val player: Player) : Player.Listener {
        override fun onPositionDiscontinuity(
            oldPosition: Player.PositionInfo,
            newPosition: Player.PositionInfo,
            reason: Int,
        ) {
            val id = oldPosition.mediaItem?.mediaId ?: return
            when {
                reason == Player.DISCONTINUITY_REASON_AUTO_TRANSITION -> MusicLibrary.played(id)
                reason == Player.DISCONTINUITY_REASON_SEEK && oldPosition.mediaItemIndex != newPosition.mediaItemIndex -> {
                    val length = MusicLibrary.state.value.songs[id]?.durationMs ?: 0L
                    if (length > 0 && oldPosition.positionMs < length / 2) MusicLibrary.skipped(id)
                }
            }
        }

        override fun onPlaybackStateChanged(state: Int) {
            if (state == Player.STATE_ENDED) player.currentMediaItem?.mediaId?.let(MusicLibrary::played)
        }
    }
}
