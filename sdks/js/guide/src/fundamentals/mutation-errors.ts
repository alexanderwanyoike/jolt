import {
  AccessRevokedError,
  ConflictError,
  ItemUnavailableError,
  SchemaValidationError,
} from "jolt-sdk/data";

export function mutationMessage(error: unknown): string {
  if (error instanceof ConflictError) {
    return "This item changed. Read it again before retrying.";
  }
  if (error instanceof ItemUnavailableError) {
    return "Jolt cannot safely change this item right now.";
  }
  if (error instanceof AccessRevokedError) {
    return "Reconnect to Jolt and request approval again.";
  }
  if (error instanceof SchemaValidationError) {
    return `Check the ${error.field} field.`;
  }
  throw error;
}
