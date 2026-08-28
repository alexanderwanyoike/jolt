import type { AppCompatibilityResult } from "./compatibility.js";
import type { AppSessionStatus } from "./wire.js";

/** The connected Jolt node cannot provide the behavior declared by this App. */
export class AppIncompatibleError extends Error {
  constructor(readonly compatibility: AppCompatibilityResult) {
    super("The connected Jolt node is incompatible with this application");
    this.name = "AppIncompatibleError";
  }
}

/** The person using Jolt did not approve the App's derived access request. */
export class AppSessionRejectedError extends Error {
  constructor(readonly status: AppSessionStatus) {
    super(`The Jolt application session was ${status}`);
    this.name = "AppSessionRejectedError";
  }
}
