// Shared status DTOs mirroring the Rust StatusDto (commands::cmd_get_status). Consumed by the
// Today's breaks dashboard for its per-card countdowns.

export interface NextBreakDto {
  rule_id: string;
  rule_name: string;
  remaining_secs: number;
}

export interface StatusDto {
  state: "stopped" | "running" | "paused" | "in_break";
  /** Every enabled break, soonest-first. */
  all: NextBreakDto[];
}
