package it.mauromazzieri.musiccraft

import android.content.ComponentName
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Favorite
import androidx.compose.material.icons.filled.FavoriteBorder
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Star
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.IconToggleButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.Player
import androidx.media3.session.MediaController
import androidx.media3.session.SessionToken

class MainActivity : ComponentActivity() {
    private var controller: MediaController? = null
    private val nowPlaying = mutableStateOf<String?>(null)
    private val playing = mutableStateOf(false)

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        MusicLibrary.start(this)
        setContent {
            val dark = isSystemInDarkTheme()
            MaterialTheme(colorScheme = if (dark) darkColorScheme(primary = Teal) else lightColorScheme(primary = Teal)) {
                Surface(Modifier.fillMaxSize()) {
                    App(
                        nowPlaying = nowPlaying.value,
                        playing = playing.value,
                        play = ::play,
                        toggle = { controller?.let { if (it.isPlaying) it.pause() else it.play() } },
                        next = { controller?.seekToNextMediaItem() },
                        previous = { controller?.seekToPreviousMediaItem() },
                    )
                }
            }
        }
    }

    override fun onStart() {
        super.onStart()
        val token = SessionToken(this, ComponentName(this, PlaybackService::class.java))
        val future = MediaController.Builder(this, token).buildAsync()
        future.addListener({
            val c = runCatching { future.get() }.getOrNull() ?: return@addListener
            controller = c
            c.addListener(object : Player.Listener {
                override fun onEvents(player: Player, events: Player.Events) = show(player)
            })
            show(c)
        }, ContextCompat.getMainExecutor(this))
    }

    override fun onResume() {
        super.onResume()
        MusicLibrary.reload()
    }

    override fun onStop() {
        controller?.release()
        controller = null
        super.onStop()
    }

    private fun show(player: Player) {
        val m = player.mediaMetadata
        nowPlaying.value = m.title?.let { t -> listOfNotNull(t, m.artist).joinToString(" · ") }
        playing.value = player.isPlaying
    }

    /** Plays `ids` from `start`, the songs that are on this phone. */
    private fun play(ids: List<String>, start: Int) {
        val c = controller ?: return
        val songs = MusicLibrary.state.value.songs
        val items = ArrayList<MediaItem>()
        var first = 0
        ids.forEachIndexed { i, id ->
            val song = songs[id] ?: return@forEachIndexed
            val uri = MusicLibrary.uriOf(song) ?: return@forEachIndexed
            if (i == start) first = items.size
            val meta = MediaMetadata.Builder()
                .setTitle(song.title)
                .setArtist(song.artist)
                .setAlbumTitle(song.album)
                .build()
            items.add(MediaItem.Builder().setMediaId(song.id).setUri(uri).setMediaMetadata(meta).build())
        }
        if (items.isEmpty()) return
        c.setMediaItems(items, first, 0)
        c.prepare()
        c.play()
    }

    companion object {
        val Teal = Color(0xFF0E8A96)
    }
}

private enum class Tab { Albums, Songs, Playlists }

private sealed interface Detail {
    data class OfAlbum(val album: Album) : Detail
    data class OfPlaylist(val playlist: PlaylistInfo) : Detail
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun App(
    nowPlaying: String?,
    playing: Boolean,
    play: (List<String>, Int) -> Unit,
    toggle: () -> Unit,
    next: () -> Unit,
    previous: () -> Unit,
) {
    val state by MusicLibrary.state.collectAsStateWithLifecycle()
    val pick = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocumentTree()) { tree ->
        if (tree != null) MusicLibrary.choose(tree)
    }
    if (!state.ready) return
    if (!state.hasFolder) {
        Welcome { pick.launch(null) }
        return
    }
    var tab by rememberSaveable { mutableStateOf(Tab.Albums) }
    var detail by remember { mutableStateOf<Detail?>(null) }
    var rating by remember { mutableStateOf<Song?>(null) }
    BackHandler(enabled = detail != null) { detail = null }

    Scaffold(
        topBar = {
            TopAppBar(
                title = {
                    Text(
                        when (val d = detail) {
                            is Detail.OfAlbum -> d.album.title
                            is Detail.OfPlaylist -> d.playlist.name
                            null -> stringResource(R.string.app_name)
                        },
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                },
                navigationIcon = {
                    if (detail != null) {
                        IconButton(onClick = { detail = null }) {
                            Icon(Icons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.back))
                        }
                    }
                },
                actions = {
                    IconButton(onClick = MusicLibrary::reload) {
                        Icon(Icons.Filled.Refresh, stringResource(R.string.reload))
                    }
                    TextButton(onClick = { pick.launch(null) }) { Text(stringResource(R.string.change_folder)) }
                },
            )
        },
        bottomBar = {
            Column {
                if (nowPlaying != null) MiniPlayer(nowPlaying, playing, toggle, next, previous)
                NavigationBar {
                    for (t in Tab.entries) {
                        NavigationBarItem(
                            selected = tab == t && detail == null,
                            onClick = { tab = t; detail = null },
                            icon = {},
                            label = {
                                Text(stringResource(when (t) {
                                    Tab.Albums -> R.string.albums
                                    Tab.Songs -> R.string.songs
                                    Tab.Playlists -> R.string.playlists
                                }))
                            },
                        )
                    }
                }
            }
        },
    ) { inner ->
        Column(Modifier.padding(inner).fillMaxSize()) {
            if (state.busy) LinearProgressIndicator(Modifier.fillMaxWidth())
            state.error?.let { Text(it, color = MaterialTheme.colorScheme.error, modifier = Modifier.padding(12.dp)) }
            if (state.warnings.isNotEmpty()) {
                Text(
                    stringResource(R.string.unreadable, state.warnings.joinToString("; ")),
                    style = MaterialTheme.typography.bodySmall,
                    modifier = Modifier.padding(horizontal = 12.dp),
                )
            }
            val onRate: (Song) -> Unit = { rating = it }
            when (val d = detail) {
                is Detail.OfAlbum -> SongList(d.album.songs, state, play, onRate)
                is Detail.OfPlaylist -> {
                    var ids by remember(d.playlist.id) { mutableStateOf<List<String>>(emptyList()) }
                    LaunchedEffect(d.playlist.id, state.songs) { ids = MusicLibrary.playlistSongs(d.playlist.id) }
                    SongList(ids, state, play, onRate)
                }
                null -> when (tab) {
                    Tab.Albums -> AlbumList(state) { detail = Detail.OfAlbum(it) }
                    Tab.Songs -> {
                        var query by rememberSaveable { mutableStateOf("") }
                        OutlinedTextField(
                            value = query,
                            onValueChange = { query = it },
                            label = { Text(stringResource(R.string.search)) },
                            singleLine = true,
                            modifier = Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 4.dp),
                        )
                        val words = query.lowercase().split(' ').filter { it.isNotBlank() }
                        val ids = state.order.filter { id ->
                            val s = state.songs[id] ?: return@filter false
                            val text = "${s.title} ${s.artist} ${s.album}".lowercase()
                            words.all { text.contains(it) }
                        }
                        SongList(ids, state, play, onRate)
                    }
                    Tab.Playlists -> PlaylistList(state) { detail = Detail.OfPlaylist(it) }
                }
            }
        }
    }
    rating?.let { song -> RatingDialog(song) { rating = null } }
}

@Composable
private fun Welcome(choose: () -> Unit) {
    Column(
        Modifier.fillMaxSize().padding(24.dp),
        verticalArrangement = Arrangement.Center,
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text(stringResource(R.string.app_name), style = MaterialTheme.typography.headlineMedium)
        Text(stringResource(R.string.welcome), modifier = Modifier.padding(vertical = 16.dp))
        Button(onClick = choose) { Text(stringResource(R.string.choose_folder)) }
    }
}

@Composable
private fun AlbumList(state: LibraryState, open: (Album) -> Unit) {
    if (state.albums.isEmpty() && !state.busy) {
        Text(stringResource(R.string.empty), modifier = Modifier.padding(24.dp))
        return
    }
    LazyColumn {
        items(state.albums) { a ->
            Row2(
                a.title.ifBlank { stringResource(R.string.unknown_album) },
                listOf(a.artist.ifBlank { stringResource(R.string.unknown_artist) }, if (a.year > 0) "${a.year}" else "")
                    .filter { it.isNotEmpty() }.joinToString(" · "),
                Modifier.combinedClickableCompat { open(a) },
            )
            HorizontalDivider()
        }
    }
}

@Composable
private fun PlaylistList(state: LibraryState, open: (PlaylistInfo) -> Unit) {
    LazyColumn {
        items(state.playlists) { p ->
            Row2(p.name, if (p.smart) stringResource(R.string.smart) else "", Modifier.combinedClickableCompat { open(p) })
            HorizontalDivider()
        }
    }
}

@Composable
private fun SongList(ids: List<String>, state: LibraryState, play: (List<String>, Int) -> Unit, rate: (Song) -> Unit) {
    LazyColumn {
        items(ids.size) { i ->
            val song = state.songs[ids[i]] ?: return@items
            SongRow(song, onPlay = { play(ids, i) }, onRate = { rate(song) })
            HorizontalDivider()
        }
    }
}

@Composable
private fun SongRow(song: Song, onPlay: () -> Unit, onRate: () -> Unit) {
    val notHere = stringResource(R.string.not_here)
    val subtitle = listOf(song.artist, song.album).filter { it.isNotBlank() }.joinToString(" · ")
    val dim = if (song.here) Color.Unspecified else MaterialTheme.colorScheme.onSurface.copy(alpha = 0.5f)
    Row(
        Modifier.fillMaxWidth()
            .combinedClickableCompat(onLongClick = onRate) { if (song.here) onPlay() }
            .semantics { if (!song.here) stateDescription = notHere }
            .padding(start = 16.dp, end = 4.dp, top = 8.dp, bottom = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) {
            Text(song.title, color = dim, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(
                if (song.here) subtitle else "$subtitle · $notHere",
                style = MaterialTheme.typography.bodySmall,
                color = dim,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
        if (song.rating > 0) Text("★".repeat(song.rating), style = MaterialTheme.typography.bodySmall)
        IconToggleButton(checked = song.loved, onCheckedChange = { MusicLibrary.setLoved(song.id, it) }) {
            Icon(
                if (song.loved) Icons.Filled.Favorite else Icons.Filled.FavoriteBorder,
                stringResource(R.string.love),
            )
        }
    }
}

@Composable
private fun Row2(title: String, subtitle: String, modifier: Modifier) {
    Column(modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 10.dp)) {
        Text(title, maxLines = 1, overflow = TextOverflow.Ellipsis)
        if (subtitle.isNotEmpty()) {
            Text(subtitle, style = MaterialTheme.typography.bodySmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
    }
}

@Composable
private fun MiniPlayer(text: String, playing: Boolean, toggle: () -> Unit, next: () -> Unit, previous: () -> Unit) {
    Surface(tonalElevation = 3.dp) {
        Row(Modifier.fillMaxWidth().padding(start = 16.dp), verticalAlignment = Alignment.CenterVertically) {
            Text(text, Modifier.weight(1f), maxLines = 1, overflow = TextOverflow.Ellipsis)
            IconButton(onClick = previous) { Icon(Glyphs.Previous, stringResource(R.string.previous)) }
            IconButton(onClick = toggle) {
                Icon(
                    if (playing) Glyphs.Pause else Glyphs.Play,
                    stringResource(if (playing) R.string.pause else R.string.play),
                )
            }
            IconButton(onClick = next) { Icon(Glyphs.Next, stringResource(R.string.next)) }
        }
    }
}

@Composable
private fun RatingDialog(song: Song, close: () -> Unit) {
    AlertDialog(
        onDismissRequest = close,
        title = { Text(song.title) },
        text = {
            Row {
                for (n in 1..5) {
                    val label = stringResource(R.string.stars, n)
                    IconButton(
                        onClick = { MusicLibrary.setRating(song.id, if (song.rating == n) 0 else n); close() },
                        modifier = Modifier.semantics { contentDescription = label },
                    ) {
                        Icon(
                            Icons.Filled.Star,
                            null,
                            tint = if (n <= song.rating) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.outline,
                        )
                    }
                }
            }
        },
        confirmButton = {
            TextButton(onClick = { MusicLibrary.setRating(song.id, 0); close() }) { Text(stringResource(R.string.no_rating)) }
        },
        dismissButton = { TextButton(onClick = close) { Text(stringResource(R.string.close)) } },
    )
}

@OptIn(ExperimentalFoundationApi::class)
private fun Modifier.combinedClickableCompat(onLongClick: (() -> Unit)? = null, onClick: () -> Unit): Modifier =
    combinedClickable(onLongClick = onLongClick, onClick = onClick)
