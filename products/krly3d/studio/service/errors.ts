import { Code, ConnectError } from "@connectrpc/connect";
import { ProviderError, type ProviderErrorCode } from "./ai/errors.ts";

const messages: Record<ProviderErrorCode, string> = {
  not_configured:
    "A kért AI- vagy beszédfunkció még nincs beállítva a szerveren.",
  invalid_configuration: "Az AI-szolgáltatás szerveroldali beállítása hibás.",
  invalid_input:
    "Az AI-bemenet nem támogatott. Képes chathez legfeljebb 8 MiB-os PNG/JPEG képet használj; PNM-ből az Építés móddal dolgozhatsz.",
  aborted: "A műveletet leállítottad.",
  timeout: "Az AI-szolgáltatás nem válaszolt a határidőn belül. Próbáld újra.",
  network:
    "Az AI-szolgáltatás nem érhető el. Ellenőrizd a LiteLLM-kapcsolatot.",
  http_error: "Az AI-szolgáltatás elutasította a kérést.",
  response_too_large: "Az AI-szolgáltatás válasza túllépte a méretkorlátot.",
  invalid_response:
    "Az AI műveletterve nem ment át az ellenőrzésen. Ebből a tervből nem épült új modell; az előző modell megmaradt.",
  no_speech: "Nem hallható felismerhető beszéd. Készíts új felvételt.",
  decoder_unavailable: "A hangátalakításhoz ffmpeg szükséges a szerveren.",
  invalid_audio: "A hangfelvételt nem sikerült olvasni. Készíts új felvételt.",
};

export function providerError(error: unknown): unknown {
  if (!(error instanceof ProviderError)) return error;
  const code = error.code === "aborted"
    ? Code.Canceled
    : error.code === "timeout"
    ? Code.DeadlineExceeded
    : ["invalid_input", "invalid_audio", "no_speech"].includes(error.code)
    ? Code.InvalidArgument
    : ["not_configured", "invalid_configuration", "decoder_unavailable"]
        .includes(error.code)
    ? Code.FailedPrecondition
    : Code.Unavailable;
  return new ConnectError(
    messages[error.code] +
      (error.httpStatus ? ` (HTTP ${error.httpStatus})` : ""),
    code,
  );
}
