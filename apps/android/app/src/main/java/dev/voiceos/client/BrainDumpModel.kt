package dev.voiceos.client

enum class BrainDumpAction { DUPLICATE, CREATE, UPDATE, IGNORE }

data class BrainDumpProposal(
    val stableId: String,
    val spokenItem: String,
    val action: BrainDumpAction,
    val existingTaskId: String? = null,
    val title: String,
    val observableOutcome: String,
    val estimatedMinutes: Int,
    val importance: String,
    val reason: String,
    val priorityChallenge: String = "",
    val selectedByDefault: Boolean = action in setOf(BrainDumpAction.CREATE, BrainDumpAction.UPDATE),
)

data class BrainDumpQuestion(val id: String, val question: String, val reason: String)

data class BrainDumpReview(
    val summary: String,
    val questions: List<BrainDumpQuestion>,
    val callouts: List<String>,
    val proposals: List<BrainDumpProposal>,
)

object BrainDumpModel {
    fun review(
        transcript: String,
        openTasks: List<VoiceTask>,
        answers: Map<String, String> = emptyMap(),
    ): BrainDumpReview {
        val items = splitItems(transcript)
        val answerText = answers.values.joinToString(" ")
        val proposals = items.mapIndexed { index, spoken -> proposal(index, spoken, openTasks, answerText) }
            .distinctBy { "${it.action}:${it.existingTaskId}:${normalize(it.title)}" }
            .take(15)
        val actionable = proposals.filter { it.action != BrainDumpAction.IGNORE }
        val callouts = buildCallouts(openTasks, actionable, answerText)
        return BrainDumpReview(
            summary = when {
                actionable.isEmpty() -> "I did not hear a clear task yet. Keep talking or type what has to change."
                actionable.size == 1 -> "I found one actionable item and checked it against your unfinished work."
                else -> "I found ${actionable.size} actionable items and checked each one against your unfinished work."
            },
            questions = buildQuestions(actionable, answers),
            callouts = callouts,
            proposals = proposals,
        )
    }

    private fun proposal(
        index: Int,
        spoken: String,
        openTasks: List<VoiceTask>,
        answerText: String,
    ): BrainDumpProposal {
        val stripped = stripLeadIn(spoken)
        if (!looksActionable(spoken, stripped)) {
            return BrainDumpProposal(
                stableId = "thought:$index",
                spokenItem = spoken,
                action = BrainDumpAction.IGNORE,
                title = stripped,
                observableOutcome = "",
                estimatedMinutes = 0,
                importance = "low",
                reason = "This sounds like a thought or concern, not yet a concrete task.",
            )
        }
        val ranked = openTasks.map { task -> task to similarity(stripped, "${task.title} ${task.observableOutcome}") }
            .sortedByDescending { it.second }
        val match = ranked.firstOrNull()
        val updateLanguage = UPDATE_MARKERS.any(spoken.lowercase()::contains)
        val action = when {
            match != null && match.second >= 0.62 -> BrainDumpAction.DUPLICATE
            match != null && match.second >= 0.34 && updateLanguage -> BrainDumpAction.UPDATE
            else -> BrainDumpAction.CREATE
        }
        val existing = match?.first?.takeIf { action in setOf(BrainDumpAction.DUPLICATE, BrainDumpAction.UPDATE) }
        val title = existing?.title ?: sentenceCase(stripped).take(100)
        val estimate = existing?.estimatedMinutes ?: estimateMinutes(stripped)
        var importance = existing?.importance?.takeIf { it in IMPORTANCE } ?: inferImportance(spoken)
        val chosenByAnswer = tokenOverlap(title, answerText) >= 0.3
        if (chosenByAnswer) importance = "high"
        val challenge = when {
            importance == "low" && estimate <= 15 -> "This may be easy and satisfying, but easy is not the same as important."
            estimate >= 60 -> "This is large enough to trigger avoidance. Choose the smallest visible first step before promoting it."
            else -> ""
        }
        return BrainDumpProposal(
            stableId = "brain:$index:${existing?.id.orEmpty()}:${normalize(title)}",
            spokenItem = spoken,
            action = action,
            existingTaskId = existing?.id,
            title = title,
            observableOutcome = if (existing != null) existing.observableOutcome else "Done: $title",
            estimatedMinutes = estimate,
            importance = importance,
            reason = when (action) {
                BrainDumpAction.DUPLICATE -> "Already unfinished on your task list. I will not create it twice."
                BrainDumpAction.UPDATE -> "This adds information or urgency to an unfinished task."
                BrainDumpAction.CREATE -> if (chosenByAnswer) "Your priority answer points to this as consequential." else "This is new, concrete, actionable work."
                BrainDumpAction.IGNORE -> "Not actionable."
            },
            priorityChallenge = challenge,
        )
    }

    private fun buildQuestions(
        proposals: List<BrainDumpProposal>,
        answers: Map<String, String>,
    ): List<BrainDumpQuestion> {
        if (proposals.isEmpty()) return emptyList()
        return buildList {
            if (proposals.size > 1 && answers["consequence"].isNullOrBlank()) add(
                BrainDumpQuestion(
                    "consequence",
                    "Which item has the biggest real consequence if it waits—and who feels that consequence?",
                    "Urgency and importance are not the same thing.",
                ),
            )
            if (proposals.any { it.estimatedMinutes >= 45 } && answers["avoidance"].isNullOrBlank()) add(
                BrainDumpQuestion(
                    "avoidance",
                    "Which large or unclear item are you most tempted to avoid because it feels heavy?",
                    "Complexity can quietly push important work behind easier tasks.",
                ),
            )
            if (answers["deadline"].isNullOrBlank()) add(
                BrainDumpQuestion(
                    "deadline",
                    "Is anyone waiting on one of these, or is there a real deadline within seven days?",
                    "A real external consequence should outrank a vague feeling of urgency.",
                ),
            )
        }.take(3)
    }

    private fun buildCallouts(
        openTasks: List<VoiceTask>,
        proposals: List<BrainDumpProposal>,
        answerText: String,
    ): List<String> = buildList {
        val mentionedIds = proposals.mapNotNull(BrainDumpProposal::existingTaskId).toSet()
        val importantOmitted = openTasks.firstOrNull {
            it.importance == "high" && it.id !in mentionedIds && it.status != "active"
        }
        if (importantOmitted != null) add(
            "I’m going to push back: “${importantOmitted.title}” is already marked high importance, but it disappeared from this dump. Are you deliberately replacing it—or avoiding it?",
        )
        val largeOmitted = openTasks.firstOrNull {
            it.estimatedMinutes >= 60 && it.id !in mentionedIds && it.status !in setOf("active", "blocked")
        }
        if (largeOmitted != null && proposals.any { it.estimatedMinutes <= 20 }) add(
            "The smaller items may feel safer, but “${largeOmitted.title}” is still waiting. Its size is a reason to shrink the next step, not a reason to keep postponing it.",
        )
        val answerChoice = proposals.maxByOrNull { tokenOverlap(it.title, answerText) }
            ?.takeIf { answerText.isNotBlank() && tokenOverlap(it.title, answerText) >= 0.3 }
        val strongerExisting = openTasks.firstOrNull { it.importance == "high" && it.id != answerChoice?.existingTaskId }
        if (answerChoice != null && answerChoice.importance == "low" && strongerExisting != null) add(
            "You named “${answerChoice.title}” first, but “${strongerExisting.title}” carries more recorded importance. Tell me what changed before we demote it.",
        )
    }.distinct().take(3)

    internal fun splitItems(transcript: String): List<String> {
        val punctuated = transcript.replace('\n', '.')
        val primary = punctuated.split(Regex("[.!?;]+"))
        return primary.flatMap { chunk ->
            chunk.split(Regex("(?i)(?=\\b(?:i also need to|i need to|i have to|i should|don't forget to|remember to|then i need to|also)\\b)"))
        }.map(String::trim)
            .filter { it.length >= 4 }
            .distinctBy(::normalize)
            .take(20)
    }

    internal fun similarity(left: String, right: String): Double {
        val leftTokens = tokens(left)
        val rightTokens = tokens(right)
        if (leftTokens.isEmpty() || rightTokens.isEmpty()) return 0.0
        if (normalize(left) == normalize(right)) return 1.0
        val intersection = leftTokens.intersect(rightTokens).size.toDouble()
        val union = leftTokens.union(rightTokens).size.toDouble()
        val containment = intersection / minOf(leftTokens.size, rightTokens.size).toDouble()
        return maxOf(intersection / union, containment * 0.88)
    }

    private fun looksActionable(original: String, stripped: String): Boolean {
        val normalized = original.lowercase()
        if (ACTION_MARKERS.any(normalized::contains)) return true
        return stripped.split(' ').size >= 2 && ACTION_VERBS.any { stripped.lowercase().startsWith(it) }
    }

    private fun stripLeadIn(value: String): String = value.trim()
        .replace(Regex("(?i)^(?:and |also |then )?(?:i need to|i have to|i should|don't forget to|remember to|please remind me to)\\s+"), "")
        .trim(' ', ',', '-')

    private fun inferImportance(value: String): String {
        val normalized = value.lowercase()
        return when {
            HIGH_MARKERS.any(normalized::contains) -> "high"
            LOW_MARKERS.any(normalized::contains) -> "low"
            else -> "normal"
        }
    }

    private fun estimateMinutes(value: String): Int {
        val normalized = value.lowercase()
        val stated = Regex("\\b(\\d{1,3})\\s*(?:minute|min)\\b").find(normalized)
            ?.groupValues?.getOrNull(1)?.toIntOrNull()
        if (stated != null) return stated.coerceIn(1, 1_440)
        return when {
            listOf("call ", "email ", "text ", "reply ", "schedule ").any(normalized::startsWith) -> 10
            listOf("buy ", "order ", "pick up ", "pay ").any(normalized::startsWith) -> 15
            listOf("review ", "check ", "confirm ").any(normalized::startsWith) -> 20
            listOf("research ", "compare ", "plan ").any(normalized::startsWith) -> 30
            listOf("build ", "create ", "write ", "organize ", "prepare ").any(normalized::startsWith) -> 45
            else -> 20
        }
    }

    private fun tokenOverlap(left: String, right: String): Double {
        if (right.isBlank()) return 0.0
        val leftTokens = tokens(left)
        if (leftTokens.isEmpty()) return 0.0
        return leftTokens.intersect(tokens(right)).size.toDouble() / leftTokens.size
    }

    private fun tokens(value: String): Set<String> = normalize(value).split(' ')
        .filter { it.length > 2 && it !in STOP_WORDS }
        .toSet()

    private fun normalize(value: String): String = value.lowercase()
        .replace(Regex("[^a-z0-9 ]"), " ")
        .replace(Regex("\\s+"), " ")
        .trim()

    private fun sentenceCase(value: String): String = value.replaceFirstChar {
        if (it.isLowerCase()) it.titlecase() else it.toString()
    }

    private val IMPORTANCE = setOf("high", "normal", "low")
    private val ACTION_MARKERS = listOf("i need", "i have to", "i should", "remind me", "don't forget", "must ")
    private val UPDATE_MARKERS = listOf("update", "change", "also", "add to", "more urgent", "deadline", "instead")
    private val HIGH_MARKERS = listOf("urgent", "critical", "must", "today", "deadline", "overdue", "important", "asap")
    private val LOW_MARKERS = listOf("maybe", "someday", "eventually", "nice to have", "when i get time")
    private val ACTION_VERBS = listOf(
        "call ", "email ", "text ", "reply ", "schedule ", "buy ", "order ", "pay ", "review ",
        "check ", "confirm ", "research ", "compare ", "plan ", "build ", "create ", "write ",
        "organize ", "prepare ", "finish ", "fix ", "send ", "submit ", "book ", "clean ",
    )
    private val STOP_WORDS = setOf(
        "the", "and", "for", "that", "this", "with", "from", "into", "need", "have", "should",
        "task", "done", "also", "then", "about", "just", "really", "want", "make", "some",
    )
}
