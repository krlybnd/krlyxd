export type ProviderErrorCode =
  | "not_configured"
  | "invalid_configuration"
  | "invalid_input"
  | "aborted"
  | "timeout"
  | "network"
  | "http_error"
  | "response_too_large"
  | "invalid_response"
  | "no_speech"
  | "decoder_unavailable"
  | "invalid_audio";

/** Safe to serialize. Never attach upstream bodies, URLs with keys, or raw causes. */
export class ProviderError extends Error {
  constructor(
    readonly code: ProviderErrorCode,
    message: string,
    readonly httpStatus?: number,
  ) {
    super(message);
    this.name = "ProviderError";
  }
}
