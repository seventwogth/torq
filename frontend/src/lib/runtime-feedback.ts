export type RuntimeActionName = 'start' | 'stop' | 'restart' | 'new_identity';

function extractMessage(error: unknown) {
  return error instanceof Error ? error.message.trim() : String(error).trim();
}

function actionLabel(action: RuntimeActionName) {
  const labels: Record<RuntimeActionName, string> = {
    start: 'Start',
    stop: 'Stop',
    restart: 'Restart',
    new_identity: 'New Identity',
  };

  return labels[action];
}

function looksLikeMissingTorBinary(normalizedMessage: string) {
  return (
    normalizedMessage.includes('failed to spawn tor process') &&
    (normalizedMessage.includes('os error 2') ||
      normalizedMessage.includes('cannot find the file') ||
      normalizedMessage.includes('cannot find the path') ||
      normalizedMessage.includes('the system cannot find') ||
      normalizedMessage.includes('not found'))
  );
}

export function humanizeRuntimeMessage(rawMessage: string) {
  const normalizedMessage = rawMessage.toLowerCase();

  if (looksLikeMissingTorBinary(normalizedMessage)) {
    return 'Tor could not be started because tor.exe was not found or could not be launched. Set TORQ_TOR_EXE if Tor is not on PATH.';
  }

  if (
    normalizedMessage.includes('failed to prepare log file') ||
    normalizedMessage.includes('failed to create log directory')
  ) {
    return 'Tor log output could not be prepared. Check TORQ_TOR_LOG and filesystem permissions.';
  }

  if (normalizedMessage.includes('controlport configuration')) {
    return 'New Identity is unavailable until ControlPort is configured.';
  }

  if (normalizedMessage.includes('failed to request new identity via controlport')) {
    return 'ControlPort is configured but not reachable for a New Identity request.';
  }

  if (normalizedMessage.includes('bootstrap observation via controlport is unavailable')) {
    return 'ControlPort is configured but not reachable for bootstrap observation.';
  }

  if (normalizedMessage.includes('tor is not running')) {
    return 'Tor is not running.';
  }

  if (normalizedMessage.includes('tor is already running')) {
    return 'Tor is already running.';
  }

  return rawMessage;
}

export function formatUiError(prefix: string, error: unknown) {
  const message = extractMessage(error);
  return message ? `${prefix} ${message}` : prefix;
}

export function formatActionError(action: RuntimeActionName, error: unknown) {
  const rawMessage = extractMessage(error);
  const prefix = `${actionLabel(action)} failed:`;

  if (rawMessage) {
    return `${prefix} ${humanizeRuntimeMessage(rawMessage)}`;
  }

  return `${prefix} The desktop backend rejected the request.`;
}
