package io.github.realfamousbae.staya.probe

import java.io.File
import java.util.UUID

/**
 * Очередь неотправленных метрик: в памяти и на диске после каждого изменения.
 *
 * Отправленное удаляется по идентификатору, поэтому записи, добавленные во время
 * отправки, не теряются. Хранятся уже закодированные строки JSON — без координат.
 * Не потокобезопасна: вызывать с одного потока (исполнитель записи).
 */
class ProbeQueue(private val file: File, private val limit: Int = 500) {
    data class Item(val id: String, val json: String)

    private val items: MutableList<Item> = load()

    val size: Int get() = items.size

    fun peek(n: Int): List<Item> = items.take(n).toList()

    fun append(json: String) {
        items.add(Item(UUID.randomUUID().toString(), json))
        // При переполнении теряем самые старые — свежие важнее для замера.
        while (items.size > limit) items.removeAt(0)
        persist()
    }

    fun remove(id: String) {
        items.removeAll { it.id == id }
        persist()
    }

    private fun load(): MutableList<Item> {
        if (!file.exists()) return mutableListOf()
        return file.readLines().mapNotNull { line ->
            val tab = line.indexOf('\t')
            if (tab <= 0) null else Item(line.substring(0, tab), line.substring(tab + 1))
        }.toMutableList()
    }

    private fun persist() {
        val tmp = File(file.path + ".tmp")
        tmp.writeText(items.joinToString("") { "${it.id}\t${it.json}\n" })
        if (!tmp.renameTo(file)) {
            file.delete()
            tmp.renameTo(file)
        }
    }
}
