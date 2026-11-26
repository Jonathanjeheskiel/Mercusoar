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
  | {
      role: "assistant";
      seat: Seat;
      hostHub: boolean;
      hubAddress: string;
      /** Typed by an assistant who joins another PC's hub. Empty when hosting. */
      passcode: string;
    };

/**
 * Lab settings file installed by the lab admins (C:\ProgramData\Mercusoar\config.json).
 * When it exists, the role is fixed and students never see the assistant option.
 * Field names match the Rust `LabConfig` struct.
 */
export type LabConfig = {
  role: "student" | "assistant";
  seat: Seat | null;
  /** Fixed PC number for a student PC. Without it the student types it. */
  pc: number | null;
  hub_address: string | null;
};

/** Student PCs are 2–39. PC 1 (front) and PC 40 (back) are admin PCs. */
export const FIRST_STUDENT_PC = 2;
export const LAST_STUDENT_PC = 39;
