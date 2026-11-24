package it.mauromazzieri.musiccraft

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.provider.Settings
import java.io.File
import java.text.Normalizer
import java.util.UUID
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject

data class Song(
    val id: String,
    val title: String,
    val artist: String,
    val album: String,
    val albumArtist: String,
    val durationMs: Long,
    val loved: Boolean,
    val rating: Int,
    /** The song's file is on this phone. */
    val here: Boolean,
    val path: String,
)

data class Album(val title: String, val artist: String, val year: Int, val songs: List<String>)

data class PlaylistInfo(val id: String, val name: String, val smart: Boolean)

data class LibraryState(
    val ready: Boolean = false,
    val hasFolder: Boolean = false,
    val busy: Boolean = false,
    val songs: Map<String, Song> = emptyMap(),
    val order: List<String> = emptyList(),
    val albums: List<Album> = emptyList(),
    val playlists: List<PlaylistInfo> = emptyList(),
    val warnings: List<String> = emptyList(),
    val error: String? = null,
)

/**
 * The library folder and its catalogue, shared by the screens and the player. All file work runs
 * on the IO dispatcher, one task at a time.
 */
object MusicLibrary {
    private const val PREFS = "musiccraft"
    private val lock = Mutex()
    val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    private val mutableState = MutableStateFlow(LibraryState())
    val state: StateFlow<LibraryState> = mutableState

    private lateinit var app: Context
    private var folder: Folder? = null
    private var files: Map<String, Folder.Entry> = emptyMap()
    /** This phone's own files that are damaged in the shared folder and could not be set aside. */
    private var blocked: Set<String> = emptySet()

    /** Opens the folder chosen before, if the phone still lets the app use it. */
    fun start(context: Context) {
        if (::app.isInitialized) return
        app = context.applicationContext
        scope.launch {
            lock.withLock {
                runCatching { Core.ask("open") { put("device", device()); put("device_name", deviceName()) } }
                    .onFailure { e -> fail(e) }
                val saved = prefs().getString("folder", null)?.let(Uri::parse)
                val allowed = saved != null && app.contentResolver.persistedUriPermissions.any { it.uri == saved && it.isReadPermission && it.isWritePermission }
                if (allowed && saved != null) {
                    folder = Folder(app.contentResolver, saved)
                    load()
                }
                mutableState.value = mutableState.value.copy(ready = true, hasFolder = folder != null)
            }
        }
    }

    /** Uses a folder the person picked. */
    fun choose(tree: Uri) {
        scope.launch {
            lock.withLock {
                val flags = Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION
                runCatching { app.contentResolver.takePersistableUriPermission(tree, flags) }
                prefs().edit().putString("folder", tree.toString()).apply()
                folder = Folder(app.contentResolver, tree)
                mutableState.value = mutableState.value.copy(hasFolder = true)
                load()
            }
        }
    }

    /** Reads the catalogue and the music folder again: other computers may have changed them. */
    fun reload() {
        scope.launch { lock.withLock { if (folder != null) load() } }
    }

    fun uriOf(song: Song): Uri? {
        val f = folder ?: return null
        return files[song.path]?.let { f.uri(it.id) }
    }

    fun setLoved(id: String, loved: Boolean) = change("set_loved") { put("id", id); put("loved", loved) }

    fun setRating(id: String, stars: Int) = change("set_rating") { put("id", id); put("stars", stars) }

    fun played(id: String) = change("played") { put("id", id) }

    fun skipped(id: String) = change("skipped") { put("id", id) }

    suspend fun playlistSongs(id: String): List<String> = withContext(Dispatchers.IO) {
        lock.withLock {
            runCatching { ids(Core.ask("playlist_songs") { put("id", id) } as JSONArray) }.getOrDefault(emptyList())
        }
    }

    private fun change(op: String, fill: JSONObject.() -> Unit) {
        scope.launch {
            lock.withLock {
                runCatching {
                    Core.ask(op, fill)
                    save()
                    refresh()
                }.onFailure { e -> fail(e) }
            }
        }
    }

    private fun load() {
        val f = folder ?: return
        mutableState.value = mutableState.value.copy(busy = true, error = null)
        runCatching {
            val libraryDir = f.dir("Library", create = true) ?: throw CoreException("the Library folder could not be made")
            f.dir("Music", create = true)
            val shared = JSONArray()
            val entries = f.children(libraryDir).filter { !it.isDir && it.name.endsWith(".json") }
            for (e in entries) {
                runCatching { f.readText(e.id) }.onSuccess { text ->
                    shared.put(JSONObject().put("name", e.name).put("text", text))
                }
            }
            val backups = JSONArray()
            backupDir().listFiles()?.filter { it.name.endsWith(".json") }?.forEach { file ->
                backups.put(JSONObject().put("name", file.name).put("text", file.readText()))
            }
            val answer = Core.ask("load") { put("files", shared); put("backups", backups) } as JSONObject
            // A damaged file of this phone's own is renamed, so that writing it again loses nothing.
            val aside = answer.optJSONArray("set_aside") ?: JSONArray()
            val stillBlocked = HashSet<String>()
            for (i in 0 until aside.length()) {
                val name = aside.optString(i)
                val entry = entries.firstOrNull { it.name == name } ?: continue
                if (!f.rename(entry.id, "$name.damaged-${System.currentTimeMillis()}")) stillBlocked.add(name)
            }
            blocked = stillBlocked
            files = f.music()
            val found = JSONArray()
            for ((path, e) in files) {
                found.put(JSONObject().put("path", Normalizer.normalize(path, Normalizer.Form.NFC)).put("size", e.size))
            }
            Core.ask("found") { put("files", found) }
            files = files.mapKeys { Normalizer.normalize(it.key, Normalizer.Form.NFC) }
            save()
            val warnings = answer.optJSONArray("warnings")?.let { w -> (0 until w.length()).map { w.optString(it) } } ?: emptyList()
            refresh(warnings)
        }.onFailure { e -> fail(e) }
        mutableState.value = mutableState.value.copy(busy = false)
    }

    /** Writes this phone's changed files, in the shared folder and in the app's own copy. */
    private fun save() {
        val f = folder ?: return
        val unsaved = Core.ask("unsaved") as? JSONArray ?: return
        if (unsaved.length() == 0) return
        val libraryDir = f.dir("Library", create = true) ?: return
        val written = JSONArray()
        for (i in 0 until unsaved.length()) {
            val file = unsaved.getJSONObject(i)
            val name = file.getString("name")
            if (name in blocked) continue
            val text = file.getString("text")
            File(backupDir(), name).writeText(text)
            f.writeText(libraryDir, name, text)
            written.put(name)
        }
        Core.ask("saved") { put("names", written) }
    }

    private fun refresh(warnings: List<String> = mutableState.value.warnings) {
        val songs = LinkedHashMap<String, Song>()
        val list = Core.ask("songs") { put("sort", "artist") } as JSONArray
        for (i in 0 until list.length()) {
            val s = list.getJSONObject(i)
            val song = Song(
                id = s.getString("id"),
                title = s.optString("title"),
                artist = s.optString("artist"),
                album = s.optString("album"),
                albumArtist = s.optString("album_artist"),
                durationMs = s.optLong("duration_ms"),
                loved = s.optBoolean("loved"),
                rating = s.optInt("rating"),
                here = s.optBoolean("here"),
                path = s.optString("path"),
            )
            songs[song.id] = song
        }
        val albums = ArrayList<Album>()
        val a = Core.ask("albums") as JSONArray
        for (i in 0 until a.length()) {
            val o = a.getJSONObject(i)
            albums.add(Album(o.optString("title"), o.optString("artist"), o.optInt("year"), ids(o.getJSONArray("songs"))))
        }
        val playlists = ArrayList<PlaylistInfo>()
        val p = Core.ask("playlists") as JSONArray
        for (i in 0 until p.length()) {
            val o = p.getJSONObject(i)
            playlists.add(PlaylistInfo(o.getString("id"), o.optString("name"), !o.isNull("smart")))
        }
        mutableState.value = mutableState.value.copy(
            songs = songs,
            order = songs.keys.toList(),
            albums = albums,
            playlists = playlists,
            warnings = warnings,
        )
    }

    private fun ids(a: JSONArray): List<String> = (0 until a.length()).map { a.optString(it) }

    private fun fail(e: Throwable) {
        mutableState.value = mutableState.value.copy(error = e.message ?: e.toString())
    }

    private fun prefs() = app.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

    private fun backupDir(): File = File(app.filesDir, "library").apply { mkdirs() }

    /** A random id made once: it names this phone's files in the shared folder. */
    private fun device(): String {
        prefs().getString("device", null)?.let { return it }
        val id = UUID.randomUUID().toString().replace("-", "")
        prefs().edit().putString("device", id).apply()
        return id
    }

    private fun deviceName(): String =
        Settings.Global.getString(app.contentResolver, Settings.Global.DEVICE_NAME) ?: Build.MODEL
}
