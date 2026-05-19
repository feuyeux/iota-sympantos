
import sqlite3
kanban = '/mnt/c/Users/feuye/AppData/Local/hermes/kanban.db'
conn = sqlite3.connect(kanban)
cur = conn.cursor()
cur.execute("SELECT name FROM sqlite_master WHERE type='table'")
print('Tables:', cur.fetchall())
cur.execute("SELECT * FROM tasks WHERE id=20")
for r in cur.fetchall():
    print('Task 20:', r)
cur.execute("SELECT sql FROM sqlite_master WHERE name='tasks'")
print('Schema:', cur.fetchall())
