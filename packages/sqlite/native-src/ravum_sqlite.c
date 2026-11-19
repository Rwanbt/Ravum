/*
 * Ravum SQLite adapter (RFC-E11-SQLITE §6, decision D-19-7).
 *
 * SQLite's own API takes output pointers (`sqlite3 **`) and destructors,
 * which the Ravum FFI boundary does not admit (abi.md ABI-3.4). This adapter
 * exposes only what it admits: scalars, opaque handles (`void *`), text as a
 * pointer + byte length, and returned NUL-terminated strings.
 *
 * A connection handle holds at most one prepared statement at a time.
 * Built with SQLite itself by tools/build_sqlite_native.py; never by the
 * Ravum toolchain (modules.md MOD-5.10).
 */
#include <stdlib.h>
#include <string.h>
#include "sqlite3.h"

typedef struct {
    sqlite3 *db;
    sqlite3_stmt *stmt;
    int rc;
} rvs_conn;

static char *rvs_copy(const char *ptr, size_t len) {
    char *text = malloc(len + 1);
    if (text == NULL) return NULL;
    memcpy(text, ptr, len);
    text[len] = '\0';
    return text;
}

/* Never NULL unless out of memory; check rvs_status() for the open result. */
void *rvs_open(const char *path, size_t len) {
    rvs_conn *conn = calloc(1, sizeof *conn);
    char *name = rvs_copy(path, len);
    if (conn == NULL || name == NULL) {
        free(conn);
        free(name);
        return NULL;
    }
    conn->rc = sqlite3_open_v2(name, &conn->db,
                               SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE, NULL);
    free(name);
    if (conn->db != NULL) sqlite3_extended_result_codes(conn->db, 1);
    return conn;
}

int rvs_status(void *handle) { return ((rvs_conn *)handle)->rc; }

const char *rvs_errmsg(void *handle) {
    rvs_conn *conn = handle;
    if (conn->db == NULL) return "out of memory";
    return sqlite3_errmsg(conn->db);
}

const char *rvs_version(void) { return sqlite3_libversion(); }

void rvs_close(void *handle) {
    rvs_conn *conn = handle;
    if (conn == NULL) return;
    if (conn->stmt != NULL) sqlite3_finalize(conn->stmt);
    if (conn->db != NULL) sqlite3_close_v2(conn->db);
    free(conn);
}

/* Prepares one statement; any previous one is finalized first. */
int rvs_prepare(void *handle, const char *sql, size_t len) {
    rvs_conn *conn = handle;
    if (conn->stmt != NULL) {
        sqlite3_finalize(conn->stmt);
        conn->stmt = NULL;
    }
    conn->rc = sqlite3_prepare_v2(conn->db, sql, (int)len, &conn->stmt, NULL);
    if (conn->rc == SQLITE_OK && conn->stmt == NULL) conn->rc = SQLITE_MISUSE;
    return conn->rc;
}

int rvs_bind_null(void *handle, int index) {
    return sqlite3_bind_null(((rvs_conn *)handle)->stmt, index);
}

int rvs_bind_int(void *handle, int index, long long value) {
    return sqlite3_bind_int64(((rvs_conn *)handle)->stmt, index, value);
}

int rvs_bind_real(void *handle, int index, double value) {
    return sqlite3_bind_double(((rvs_conn *)handle)->stmt, index, value);
}

int rvs_bind_text(void *handle, int index, const char *ptr, size_t len) {
    return sqlite3_bind_text(((rvs_conn *)handle)->stmt, index, ptr, (int)len,
                             SQLITE_TRANSIENT);
}

/* 100 = a row is ready, 101 = done, anything else is an error code. */
int rvs_step(void *handle) {
    rvs_conn *conn = handle;
    conn->rc = sqlite3_step(conn->stmt);
    return conn->rc;
}

int rvs_column_count(void *handle) {
    return sqlite3_column_count(((rvs_conn *)handle)->stmt);
}

/* 1 integer, 2 real, 3 text, 4 blob, 5 null (SQLite's own codes). */
int rvs_column_type(void *handle, int column) {
    return sqlite3_column_type(((rvs_conn *)handle)->stmt, column);
}

long long rvs_column_int(void *handle, int column) {
    return sqlite3_column_int64(((rvs_conn *)handle)->stmt, column);
}

double rvs_column_real(void *handle, int column) {
    return sqlite3_column_double(((rvs_conn *)handle)->stmt, column);
}

/* Valid until the next step, reset or finalize; Ravum copies it at once. */
const char *rvs_column_text(void *handle, int column) {
    return (const char *)sqlite3_column_text(((rvs_conn *)handle)->stmt, column);
}

int rvs_finalize(void *handle) {
    rvs_conn *conn = handle;
    int rc = SQLITE_OK;
    if (conn->stmt != NULL) rc = sqlite3_finalize(conn->stmt);
    conn->stmt = NULL;
    return rc;
}

long long rvs_changes(void *handle) {
    return sqlite3_changes64(((rvs_conn *)handle)->db);
}
