Team name: [Your registered team name]
Project title: Mercusoar: Anonymous, Consent-Based Help for Computer Lab Sessions
Selected case: Case 1: Smarter Campus and Learning
Members: [Full name] (leader), [Full name], [Full name], [Full name]

Abstract

1. Problem. In computer lab sessions, students stuck on an error or task step often do not call the lab assistant, because they do not want classmates to see that they need help. Those who ask wait without knowing their place, while assistants walk from PC to PC answering the same question repeatedly. Silent students fall behind and leave tasks unfinished.

2. Intended Users. BINUS University Computer Science students in their first to third semesters attending practicum sessions in computer labs, and the lab assistants who teach them.

3. Supporting Observations or Evidence. [Interview: We interviewed N lab assistants, who reported being called about X times per session, often for repeated questions.] [Survey: Of N Computer Science students surveyed, X had been stuck in a lab without calling the assistant, mainly because Y.] We also observe classmates staying silent instead of raising a hand.

4. Proposed Solution and Core User Flow. Mercusoar is a desktop application with a Student client on each lab PC and an Assistant console. A stuck student presses a help button and may tag the task number. The request appears on the assistant's lab map in a fair queue, identified only by PC number and hidden from other students. The assistant asks to view the screen; the student approves, and an indicator stays visible. The assistant sends a private hint or, with permission, takes remote control to show the fix. Each request is logged, and after the session the assistant sees which tasks caused the most requests. The first version targets one lab room on a local network.

5. How It Differs from Existing Alternatives. Classroom management tools such as Veyon and NetSupport School let instructors monitor and control student screens, built around supervision. Mercusoar is built around help started by the student: screens open only after consent, requests stay anonymous to peers, help follows a fair queue, and session data shows where students struggle.

6. Planned Technologies. A Windows desktop application built with Tauri, using Rust for screen capture, input control, and local-network communication, with a web-based interface and SQLite logs. The first version uses no AI, since the core need is fast, private communication. Demonstrations will use synthetic data on team-owned laptops.

7. Expected Benefit and Development Plan. Expected benefit: more stuck students receive help with shorter waits, measured by help requests, waiting time, and assistant feedback during a trial session. Plan: weeks 1-2 help request, lab map, and queue; weeks 3-4 consent flow, screen viewing, and remote control; week 5 session summary and a trial on a mini lab of laptops. The main risk is lab firewall and antivirus policy, addressed by testing on our own mini lab and requesting official permission before any real-lab trial.

Source Acknowledgments
[1] [Interview with N BINUS lab assistants, date.]
[2] [Team survey of N BINUS Computer Science students, date.]
