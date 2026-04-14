# HALLAZGOS DE CÓDIGO DETALLADOS - RESTUI

## 1. INSTANCIAS DE `unwrap()` Y `expect()` CRÍTICAS

Se encontraron **204 instancias** de `unwrap/expect/panic` en el codebase.

### Ubicaciones de Mayor Riesgo:

```rust
// src/app/collections.rs:44
let last_match = *matching.last().unwrap();  // ← RIESGO: Si matching.is_empty()
```

**Problema**: Si matching es vacío (teóricamente imposible debido al filter), crash.
**Fix**: Usar `.iter()` directamente sin `.last()`

---

```rust
// src/ui/layout.rs:145-147
let block = Block::default();
// ... más código sin validación de tamaño
.split(popup_layout[1])[1]  // ← RIESGO: Array indexing sin bounds check
```

**Problema**: Si layout no tiene suficientes chunks, panic.

---

## 2. ANÁLISIS DE ESTADO COMPARTIDO

### Estado Global Complejo (`src/core/state.rs`)

```rust
pub struct AppState {
    pub request_edit: RequestEditState,
    pub body_vim: VimEditor,              // ← DUPLICADO en response_view
    pub response_view: ResponseView {
        pub resp_vim: VimEditor,          // ← DUPLICADO
        pub type_vim: VimEditor,          // ← TERCERO
    }
    pub body_type: BodyType,
    pub body_visible_width: usize,        // ← Manual sync necesaria
    pub response_view: {
        pub resp_visible_width: usize,    // ← Manual sync
    }
}
```

**Problema**: 3 instancias de `VimEditor` con **sincronización manual**

Si uno de los `visible_height` no se actualiza → **mismatch con UI**

---

## 3. RACE CONDITIONS POTENCIALES

### Request Abort Handle

```rust
// src/app/mod.rs (execute.rs:96-102)
let handle = tokio::spawn(async move { ... });
self.state.request_abort_handle = Some(handle.abort_handle());
```

**Escenario**:
1. User presiona Ctrl+R (execute)
2. Antes de que terminar, presiona Ctrl+R nuevamente
3. `if let Some(handle) = self.state.request_abort_handle.take()` → aborts anterior
4. Pero el nuevo request está ya en flight

**Mitigación**: ✓ OK - `take()` garantiza single owner

---

### Response Cache Limpieza

```rust
// src/app/execute.rs:10-22
pub(super) fn cache_response(&mut self, key: String, response: Response) {
    self.state.response_cache.insert(key, (response, std::time::Instant::now()));
    while self.state.response_cache.len() > RESPONSE_CACHE_MAX {
        if let Some(oldest_key) = self.state.response_cache.iter()
            .min_by_key(|(_, (_, ts))| *ts)
            .map(|(k, _)| k.clone())
        {
            self.state.response_cache.remove(&oldest_key);
        } else {
            break;
        }
    }
}
```

**Problema**:
- Si `RESPONSE_CACHE_MAX = 50`, puede crecer a 51 temporalmente
- Si dos requests finalizan simultáneamente: ambas llaman a `cache_response()`
  → posible double-insert

**Mitigación**: ✗ NO - Sin mutex/lock

**Recomendación**: Usar `tokio::sync::Mutex` si parallelismo es posible

---

## 4. MEMORY LEAKS POTENCIALES

### Undo Stack Sin Limite

```rust
// src/core/state.rs:18
pub const UNDO_STACK_MAX: usize = 100;

// Pero NO hay implementación de límite visible en app
// Los campos undo/redo se manejan en vimltui (externo)
```

**Verificación**: vimltui es crate externo, responsabilidad delegada ✓

---

### Response Formatting Cache

```rust
// src/ui/response.rs (no visible cache para formatted_body())
// Cada call a resp.formatted_body() potencialmente reformatea
```

**Problema**: Si response es 100MB, y user hace search → reformat completo

---

## 5. SINCRONIZACIÓN CURSOR - ANÁLISIS DETALLADO

### Header Edit - Múltiples Cursores

```rust
// src/app/inline_edit.rs:34-49
RequestFocus::Header(idx) => {
    if let Some(h) = self.state.current_request.headers.get_mut(idx) {
        let field = if self.state.request_edit.header_edit_field == 0 
            { &mut h.name } else { &mut h.value };
        
        let cursor = self.state.request_edit.header_edit_cursor
            .min(field.len());
        field.insert(cursor, c);
        self.state.request_edit.header_edit_cursor = cursor + 1;
        
        // Update autocomplete if editing header name
        if self.state.request_edit.header_edit_field == 0 {
            if let Some(h) = self.state.current_request.headers.get(idx) {
                let ac = crate::core::state::Autocomplete::new(&h.name);
                self.state.autocomplete = if ac.is_empty() { None } else { Some(ac) };
            }
        }
    }
}
```

**Estado Rastreado**:
- `header_edit_field` (0=name, 1=value)
- `header_edit_cursor` (posición)
- Actual data: `h.name` / `h.value`

**Puntos de Fallo**:

| Escenario | Posible Bug |
|-----------|-----------|
| Field vacío, cursor = 5 | `cursor.min(0)` = 0 ✓ |
| Pega con Ctrl+V (no implementado aquí) | Cursor no actualiza |
| Borra todo (dd) | field.len() = 0, cursor quedaría > 0 ❌ |

**Fix Necesario**:
```rust
// Después de cualquier modificación:
self.state.request_edit.header_edit_cursor = 
    self.state.request_edit.header_edit_cursor.min(field.len());
```

---

## 6. KEYBOARD HANDLING EDGE CASES

### Pending Key Timeout

```rust
// src/app/mod.rs:188-192
if let Some((_, instant)) = self.state.pending_key {
    if instant.elapsed() > PENDING_KEY_TIMEOUT {  // 500ms
        self.state.pending_key = None;
    }
}
```

**Edge Case**: 
- User tipea `d` (pending delete operator)
- Pausa 600ms (thinking)
- Tipea `w` (delete word)
- **Resultado**: `d` fue limpiado, `w` se procesa como comando normal

**Bueno**: Vim esperado, pero no documentado en UI

---

### Escape Key Behavior

```rust
// src/keybindings/mod.rs (no encontrado en análisis anterior)
// Pero en help: "Esc — Return to normal mode"
```

**Pregunta**: ¿Qué pasa si:
1. User está en Insert mode
2. Presiona Esc → Exit insert
3. ¿Vuelve a Normal en MISMO panel o panel anterior?

**Respuesta**: Mismos panel (Esc solo cambia modo, no navegación) ✓

---

## 7. ERROR HANDLING GAPS

### Chain Resolution Sin Logging

```rust
// src/app/execute.rs:56-63
match self.resolve_chains_in_request(&mut resolved, &mut resolving_stack).await {
    Ok(()) => {}
    Err(err) => {
        self.state.request_in_flight = false;
        self.state.last_error = Some(err.clone());
        self.state.set_status(format!("Error: {}", err));
        return;
    }
}
```

**Bueno**: Error message mostrado
**Pero**: Sin indicador del request que falló en chain
**Usuario tipico**: "¿Qué request no pude encontrar?"

---

### Timeout Sin Countdown

```rust
// src/core/http_client.rs:25
.timeout(Duration::from_secs(config.timeout_secs))
```

**Default**: Unknown (no encontrado en config.rs)
**Problema**: User no sabe cuándo timeout ocurrirá

---

## 8. PERFORMANCE PROFILES

### Search Regex vs String Find

```rust
// Actual implementación (search.rs):
line_lower[start..].find(&query)  // ← O(nm) substring search

// Más óptimo sería:
use regex::Regex;
let re = Regex::new(&regex::escape(&query))?;
re.find_iter(&line_lower)  // ← O(n) con compilación previa
```

**Pero**: Regex overhead podría no valer la pena para strings pequeños

---

### Variable Resolution Linear

```rust
// src/model/environment.rs:36-45
for (key, value) in &env.variables {
    result = result.replace(
        &format!("{{{{{}}}}}", key),  // Format new string CADA iteración
        value
    );
}
```

**Mejor**:
```rust
let formatted_key = format!("{{{{{}}}}}", key);
for _ in 0..50 {
    result = result.replace(&formatted_key, value);
}
```

---

## 9. ESTADO SERIALIZACIÓN

### Persistencia de Request

```rust
// No encontrada serialización de collections a disk
// Las collections se cargan pero NO se persisten
```

**Verificación necesaria**: ¿Dónde se guardan requests editados?
- En file de .http directamente? → Requiere parsing/serialización
- En memoria temporalmente? → Se pierden al cerrar

---

## 10. VIMLTUI DEPENDENCY RISKS

### External Cursor Shape

```rust
// src/app/mod.rs:112-115
match self.state.body_vim.cursor_shape() {
    vimltui::CursorShape::Bar => SetCursorStyle::SteadyBar,
    vimltui::CursorShape::Underline => SetCursorStyle::SteadyUnderScore,
    vimltui::CursorShape::Block => SetCursorStyle::SteadyBlock,
}
```

**Riesgo**: Si vimltui añade nuevo CursorShape → match no exhaustivo

---

## 11. TERMINAL CAPABILITY ASSUMPTIONS

```rust
// src/core/tui.rs:23-26
let _ = execute!(
    io::stdout(),
    PushKeyboardEnhancementFlags(
        KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
    )
);
```

**Problema**: Silenciosamente falla si terminal no soporta
**Síntoma**: Keys como Ctrl+Delete pueden no funcionar

---

## 12. UTF-8 Y UNICODE

### Cursor Posición en Multibyte Chars

```rust
// src/app/inline_edit.rs:23-25
let pos = row_col_to_offset(body, self.state.body_vim.cursor_row, 
                             self.state.body_vim.cursor_col);
body.insert(pos, c);
```

**Problema**:
- Si `c` es emoji (4 bytes)
- Y `cursor_col` está en middle de emoji previo
- → Posición INVÁLIDA

**Ejemplo**:
```
Body: "hello 👋 world"
      012345 67 89...
Cursor at col=6 (middle de emoji)
Insert 'x' → "hello x👋 world"  // ❌ Broken emoji
```

**Fix Necesario**: Usar grapheme clusters, no bytes

---

# MATRIZ DE PRIORIDADES DE FIXES

| Problema | Severidad | Esfuerzo | ROI |
|----------|-----------|----------|-----|
| Debounce búsqueda | Alta | Bajo | Alto |
| Cursor sync en headers | Alta | Medio | Alto |
| Validación variables | Alta | Bajo | Alto |
| Cache JSON paths | Media | Medio | Medio |
| Color contrast | Media | Bajo | Bajo |
| Emoji/Unicode cursor | Media | Medio | Medio |
| Response size limit | Alta | Bajo | Alto |
| Dirtt flag layouts | Media | Bajo | Bajo |
| Mutex response cache | Baja | Bajo | Bajo |
| Index bounds check | Baja | Bajo | Bajo |

---

