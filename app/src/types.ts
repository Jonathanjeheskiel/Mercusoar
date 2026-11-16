// Shared types for the Mercusoar screens.

/** Which admin seat a lab assistant sits at. */
export type Seat = "Front" | "Back";

/**
 * What the setup screen produces.
 * The `role` field decides which other fields exist:
 * a student has a PC number, an assistant has a seat.
 */
export type SetupConfig =
  | { role: "student"; pc: number; hubAddress: string }
  | { role: "assistant"; seat: Seat; hostHub: boolean; hubAddress: string };

/** Student PCs are 2–39. PC 1 (front) and PC 40 (back) are admin PCs. */
export const FIRST_STUDENT_PC = 2;
export const LAST_STUDENT_PC = 39;
