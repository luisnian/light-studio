-- RawStudioPDI · esquema D1 (v1)
-- Aplicar: wrangler d1 execute rawstudiopdi --remote --file=schema.sql

CREATE TABLE IF NOT EXISTS pois (
  id         TEXT PRIMARY KEY,          -- UUID v4 creado en el móvil (o en el Mac)
  seq        INTEGER NOT NULL,          -- secuencia del servidor: último cambio de este PDI
  version    INTEGER NOT NULL,          -- sube 1 con cada cambio aceptado
  data       TEXT NOT NULL,             -- JSON del PDI (contrato v1, sin seq/version)
  updated_at TEXT NOT NULL,             -- UTC, puesto por el servidor
  deleted_at TEXT                       -- borrado lógico
);
CREATE INDEX IF NOT EXISTS idx_pois_seq ON pois(seq);

-- Contador global de secuencia (cursor de sincronización)
CREATE TABLE IF NOT EXISTS seq_counter (
  id    INTEGER PRIMARY KEY CHECK (id = 1),
  value INTEGER NOT NULL
);
INSERT OR IGNORE INTO seq_counter (id, value) VALUES (1, 0);

-- Operaciones ya procesadas: reintentos seguros (idempotencia)
CREATE TABLE IF NOT EXISTS ops (
  op_id      TEXT PRIMARY KEY,
  result     TEXT NOT NULL,             -- JSON del resultado devuelto la primera vez
  created_at TEXT NOT NULL
);

-- Último cursor confirmado por cada dispositivo (móvil, Mac)
CREATE TABLE IF NOT EXISTS devices (
  device     TEXT PRIMARY KEY,
  cursor     INTEGER NOT NULL,
  updated_at TEXT NOT NULL
);

-- Fotos y adjuntos guardados en R2
CREATE TABLE IF NOT EXISTS attachments (
  key        TEXT PRIMARY KEY,          -- poi/<poi_id>/<uuid>.<ext>
  poi_id     TEXT NOT NULL,
  type       TEXT NOT NULL,
  bytes      INTEGER NOT NULL,
  created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_att_poi ON attachments(poi_id);

-- Face ID / passkeys (WebAuthn): claves públicas de los dispositivos autorizados
CREATE TABLE IF NOT EXISTS credentials (
  id         TEXT PRIMARY KEY,          -- credentialId (base64url)
  public_key TEXT NOT NULL,             -- SPKI P-256 (base64url)
  label      TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL,
  last_used  TEXT
);
-- Retos de un solo uso (2 min)
CREATE TABLE IF NOT EXISTS challenges (
  challenge  TEXT PRIMARY KEY,
  kind       TEXT NOT NULL,             -- register | login
  expires    INTEGER NOT NULL           -- ms epoch
);
