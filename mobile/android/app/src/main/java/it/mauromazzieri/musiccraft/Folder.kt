package it.mauromazzieri.musiccraft

import android.content.ContentResolver
import android.net.Uri
import android.provider.DocumentsContract
import android.provider.DocumentsContract.Document
import java.io.IOException

/**
 * The library folder the person chose, reached through the system's document access: it may be
 * on the phone, on a memory card, or in a sync app that offers its folders to other apps.
 */
class Folder(private val resolver: ContentResolver, val tree: Uri) {
    data class Entry(val id: String, val name: String, val isDir: Boolean, val size: Long)

    val rootId: String = DocumentsContract.getTreeDocumentId(tree)

    fun uri(id: String): Uri = DocumentsContract.buildDocumentUriUsingTree(tree, id)

    fun children(parentId: String): List<Entry> {
        val list = DocumentsContract.buildChildDocumentsUriUsingTree(tree, parentId)
        val columns = arrayOf(
            Document.COLUMN_DOCUMENT_ID,
            Document.COLUMN_DISPLAY_NAME,
            Document.COLUMN_MIME_TYPE,
            Document.COLUMN_SIZE,
        )
        val out = ArrayList<Entry>()
        resolver.query(list, columns, null, null, null)?.use { c ->
            while (c.moveToNext()) {
                val id = c.getString(0) ?: continue
                val name = c.getString(1) ?: continue
                val dir = c.getString(2) == Document.MIME_TYPE_DIR
                val size = if (c.isNull(3)) 0L else c.getLong(3)
                out.add(Entry(id, name, dir, size))
            }
        }
        return out
    }

    fun child(parentId: String, name: String): Entry? = children(parentId).firstOrNull { it.name == name }

    /** The id of a folder right below the library folder, made when [create] and missing. */
    fun dir(name: String, create: Boolean): String? {
        child(rootId, name)?.takeIf { it.isDir }?.let { return it.id }
        if (!create) return null
        val made = DocumentsContract.createDocument(resolver, uri(rootId), Document.MIME_TYPE_DIR, name) ?: return null
        return DocumentsContract.getDocumentId(made)
    }

    fun readText(id: String): String =
        resolver.openInputStream(uri(id))?.use { it.readBytes().toString(Charsets.UTF_8) }
            ?: throw IOException("the file could not be opened")

    /** Replaces a file's content, or makes the file. */
    fun writeText(dirId: String, name: String, text: String) {
        val target = child(dirId, name)?.let { uri(it.id) }
            ?: DocumentsContract.createDocument(resolver, uri(dirId), "application/json", name)
            ?: throw IOException("$name could not be made")
        resolver.openOutputStream(target, "wt")?.use { it.write(text.toByteArray(Charsets.UTF_8)) }
            ?: throw IOException("$name could not be written")
    }

    fun rename(id: String, name: String): Boolean =
        runCatching { DocumentsContract.renameDocument(resolver, uri(id), name) != null }.getOrDefault(false)

    /** Every file below `Music/`, by its path there, parts separated by `/`. */
    fun music(): Map<String, Entry> {
        val out = HashMap<String, Entry>()
        val top = dir("Music", create = false) ?: return out
        fun walk(id: String, prefix: String, depth: Int) {
            if (depth > 24) return
            for (e in children(id)) {
                if (e.name.startsWith(".")) continue
                val path = if (prefix.isEmpty()) e.name else "$prefix/${e.name}"
                if (e.isDir) walk(e.id, path, depth + 1) else out[path] = e
            }
        }
        walk(top, "", 0)
        return out
    }
}
