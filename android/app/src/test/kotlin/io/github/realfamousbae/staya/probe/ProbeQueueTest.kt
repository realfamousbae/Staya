package io.github.realfamousbae.staya.probe

import java.io.File
import java.nio.file.Files
import org.junit.Assert.assertEquals
import org.junit.Test

class ProbeQueueTest {
    private fun file(): File = Files.createTempDirectory("q").resolve("queue.txt").toFile()

    @Test
    fun appendDuringSendSurvivesAndPersists() {
        val f = file()
        val q = ProbeQueue(f)
        q.append("""{"n":"first"}""")
        val inFlight = q.peek(5) // отправка началась
        q.append("""{"n":"arrived-during-send"}""") // новая запись, пока ждём сеть
        inFlight.forEach { q.remove(it.id) } // отправка закончилась
        assertEquals(listOf("""{"n":"arrived-during-send"}"""), q.peek(10).map { it.json })
        // Перезапуск процесса.
        assertEquals(listOf("""{"n":"arrived-during-send"}"""), ProbeQueue(f).peek(10).map { it.json })
    }

    @Test
    fun dropsOldestOverLimit() {
        val q = ProbeQueue(file(), limit = 3)
        repeat(5) { q.append("""{"n":$it}""") }
        assertEquals(listOf("""{"n":2}""", """{"n":3}""", """{"n":4}"""), q.peek(10).map { it.json })
    }
}
