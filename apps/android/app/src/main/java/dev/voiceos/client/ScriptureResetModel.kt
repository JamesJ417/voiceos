package dev.voiceos.client

import java.time.LocalDate

data class ScripturePassage(
    val reference: String,
    val bibleCode: String,
    val theme: String,
    val noticePrompt: String,
    val actionPrompt: String,
) {
    val csbUrl: String get() = "https://www.bible.com/bible/1713/$bibleCode.CSB"
}

object ScriptureResetModel {
    private val passages = listOf(
        ScripturePassage(
            "Proverbs 3:5–6", "PRO.3.5-6", "Trust when the whole path is not visible",
            "Where are you leaning hardest on your own understanding right now?",
            "What would trusting God change about your next small action?",
        ),
        ScripturePassage(
            "Philippians 4:4–9", "PHP.4.4-9", "Prayer, attention, and peace",
            "Which word or instruction in this passage catches your attention?",
            "What anxious thought could you turn into a specific prayer today?",
        ),
        ScripturePassage(
            "Matthew 6:25–34", "MAT.6.25-34", "Today’s needs instead of tomorrow’s worry",
            "What concern about tomorrow is consuming attention meant for today?",
            "What does seeking what matters first look like in the next ten minutes?",
        ),
        ScripturePassage(
            "Psalm 46:1–10", "PSA.46.1-10", "Stillness in the middle of pressure",
            "What feels noisy, unstable, or difficult to hold right now?",
            "What might it look like to be still before taking the next action?",
        ),
        ScripturePassage(
            "James 1:2–8", "JAS.1.2-8", "Wisdom and steadiness under pressure",
            "Where do you most need wisdom rather than a quick reaction?",
            "What faithful next step can you take without having every answer?",
        ),
        ScripturePassage(
            "Romans 12:1–2", "ROM.12.1-2", "Renewing the patterns of your mind",
            "Which thought pattern is shaping you in an unhelpful direction?",
            "What true and constructive thought could replace it today?",
        ),
        ScripturePassage(
            "Colossians 3:12–17", "COL.3.12-17", "Character before productivity",
            "Which quality in this passage do you most need to put on today?",
            "How could that quality shape one conversation or task?",
        ),
        ScripturePassage(
            "Isaiah 40:28–31", "ISA.40.28-31", "Strength for the tired and overwhelmed",
            "Where are you exhausted from trying to carry everything yourself?",
            "What can be slowed down, surrendered, or approached with renewed hope?",
        ),
        ScripturePassage(
            "Luke 10:38–42", "LUK.10.38-42", "Choosing what matters amid distraction",
            "What many things are pulling your attention in different directions?",
            "What is the one necessary thing to give your attention to now?",
        ),
        ScripturePassage(
            "Psalm 139:23–24", "PSA.139.23-24", "Honest examination and direction",
            "What thought, fear, or motive are you willing to examine honestly?",
            "What better way do you want God to lead you toward today?",
        ),
        ScripturePassage(
            "1 Peter 5:6–9", "1PE.5.6-9", "Releasing anxiety and staying alert",
            "What burden are you carrying as if it depends entirely on you?",
            "What would casting that concern on God free you to do faithfully?",
        ),
        ScripturePassage(
            "Galatians 6:7–10", "GAL.6.7-10", "Not growing weary in doing good",
            "Where are you tempted to quit because progress feels too slow?",
            "What small good seed can you plant today without demanding an immediate result?",
        ),
        ScripturePassage(
            "Hebrews 12:1–3", "HEB.12.1-3", "Laying aside weight and continuing",
            "What weight—not necessarily a wrong thing—is slowing you down?",
            "What is the next part of your race that deserves patient attention?",
        ),
        ScripturePassage(
            "Micah 6:6–8", "MIC.6.6-8", "Justice, faithfulness, and humility",
            "Which part of this simple calling challenges your current priorities?",
            "What humble and faithful action can you take today?",
        ),
    )

    fun passageFor(date: LocalDate = LocalDate.now()): ScripturePassage {
        val index = Math.floorMod(date.toEpochDay(), passages.size.toLong()).toInt()
        return passages[index]
    }

    fun conversationPrompt(passage: ScripturePassage, thoughts: String): String =
        conversationPrompt(passage.reference, thoughts)

    fun conversationPrompt(reference: String, thoughts: String): String {
        val cleanThoughts = thoughts.trim().take(4_000)
        return if (cleanThoughts.isBlank()) {
            "I am doing my 10-minute VIC focus reset. I read $reference in the CSB. " +
                "Ask me one thoughtful, open-ended question about what stood out, what I am wrestling with, or how the passage meets my day. " +
                "Wait for my answer before helping me connect the reflection to one priority and one small next action. " +
                "Be curious and grounded; do not claim to speak for God or tell me that God revealed a personal command to you."
        } else {
            "I am doing my 10-minute VIC focus reset. I read $reference in the CSB. " +
                "My private reflection is: $cleanThoughts\n\n" +
                "Respond briefly, then ask exactly one thoughtful follow-up question that helps me examine the passage and my reflection more deeply. " +
                "Wait for my answer before helping me connect it to one priority and one small next action. " +
                "Be curious and grounded; do not claim to speak for God or tell me that God revealed a personal command to you."
        }
    }
}
