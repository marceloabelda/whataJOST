use super::*;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ProcessUsage {
    pub(crate) pid: u32,
    pub(crate) name: String,
    pub(crate) memory_kb: u64,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ResourceUsage {
    /// false fuera de Linux (no hay /proc)
    pub(crate) available: bool,
    /// Estilo `top`: 100 = un núcleo completo. None en la primera lectura.
    pub(crate) cpu_percent: Option<f64>,
    pub(crate) cpus: usize,
    pub(crate) memory_kb: u64,
    pub(crate) processes: Vec<ProcessUsage>,
}

/// Uso de CPU y memoria de WhataJOST y todos sus procesos hijos (WebKit web,
/// red, etc.). Lo consulta la pestaña "Recursos" de la ventana de logs solo
/// mientras está abierta: no hay ningún muestreo en segundo plano.
#[tauri::command]
pub(crate) fn get_resource_usage() -> ResourceUsage {
    #[cfg(target_os = "linux")]
    {
        linux::sample()
    }
    #[cfg(not(target_os = "linux"))]
    {
        ResourceUsage { available: false, cpu_percent: None, cpus: 0, memory_kb: 0, processes: Vec::new() }
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::{ProcessUsage, ResourceUsage};
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::time::Instant;

    /// /proc/<pid>/stat expresa los tiempos en USER_HZ, fijo en 100 en la ABI
    /// de Linux (independiente del HZ del kernel).
    const USER_HZ: f64 = 100.0;

    /// Última lectura: momento y ticks de CPU acumulados por pid.
    static LAST: Mutex<Option<(Instant, HashMap<u32, u64>)>> = Mutex::new(None);

    /// (ppid, utime + stime) de /proc/<pid>/stat. El nombre del proceso va
    /// entre paréntesis y puede tener espacios: se parsea desde el último ')'.
    fn read_stat(pid: u32) -> Option<(u32, u64)> {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let rest = &stat[stat.rfind(')')? + 2..];
        let fields: Vec<&str> = rest.split_whitespace().collect();
        // rest empieza en el campo 3 (state): ppid es el 4, utime el 14, stime el 15
        let ppid = fields.get(1)?.parse().ok()?;
        let utime: u64 = fields.get(11)?.parse().ok()?;
        let stime: u64 = fields.get(12)?.parse().ok()?;
        Some((ppid, utime + stime))
    }

    fn read_pss_kb(pid: u32) -> u64 {
        std::fs::read_to_string(format!("/proc/{pid}/smaps_rollup"))
            .ok()
            .and_then(|s| {
                s.lines()
                    .find(|l| l.starts_with("Pss:"))
                    .and_then(|l| l.split_whitespace().nth(1))
                    .and_then(|v| v.parse().ok())
            })
            .unwrap_or(0)
    }

    fn display_name(pid: u32) -> String {
        let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default();
        match comm.trim() {
            "whatajost" => "WhataJOST".to_string(),
            "WebKitWebProces" => "WebKit web".to_string(),
            "WebKitNetworkPr" => "WebKit red".to_string(),
            "WebKitGPUProces" => "WebKit GPU".to_string(),
            other => other.to_string(),
        }
    }

    /// Este proceso y todos sus descendientes.
    fn process_tree() -> Vec<u32> {
        let me = std::process::id();
        let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
        if let Ok(entries) = std::fs::read_dir("/proc") {
            for entry in entries.flatten() {
                let Some(pid) = entry.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else { continue };
                if let Some((ppid, _)) = read_stat(pid) {
                    children.entry(ppid).or_default().push(pid);
                }
            }
        }
        let mut tree = vec![me];
        let mut i = 0;
        while i < tree.len() {
            if let Some(kids) = children.get(&tree[i]) {
                tree.extend(kids);
            }
            i += 1;
        }
        tree
    }

    pub(super) fn sample() -> ResourceUsage {
        let now = Instant::now();
        let mut ticks: HashMap<u32, u64> = HashMap::new();
        let mut processes = Vec::new();
        for pid in process_tree() {
            let Some((_, t)) = read_stat(pid) else { continue };
            ticks.insert(pid, t);
            processes.push(ProcessUsage { pid, name: display_name(pid), memory_kb: read_pss_kb(pid) });
        }
        processes.sort_by_key(|p| std::cmp::Reverse(p.memory_kb));

        let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
        // Diferencia por pid: un proceso que murió no resta y uno nuevo
        // cuenta desde cero (solo su consumo desde que existe).
        let cpu_percent = last.as_ref().map(|(then, prev)| {
            let elapsed = now.duration_since(*then).as_secs_f64().max(0.001);
            let delta: u64 = ticks
                .iter()
                .map(|(pid, t)| t.saturating_sub(*prev.get(pid).unwrap_or(&0)))
                .sum();
            delta as f64 / USER_HZ / elapsed * 100.0
        });
        *last = Some((now, ticks));

        ResourceUsage {
            available: true,
            cpu_percent,
            cpus: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
            memory_kb: processes.iter().map(|p| p.memory_kb).sum(),
            processes,
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    #[test]
    fn mide_el_proceso_actual() {
        let first = super::get_resource_usage();
        assert!(first.available);
        assert!(first.processes.iter().any(|p| p.pid == std::process::id()));
        assert!(first.memory_kb > 0);
        let second = super::get_resource_usage();
        assert!(second.cpu_percent.is_some_and(|c| c >= 0.0));
    }
}
