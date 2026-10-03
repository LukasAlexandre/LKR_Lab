//! Topologia de GPUs: GPU física × display virtual (indirect) × software.
//!
//! Regressão do bug achado ao validar o Machine Health em outra workstation: uma GPU física
//! apareceu como quatro porque o Windows registra um adaptador DirectX (com LUID próprio) para
//! cada display virtual "MS Idd Device", todos descritos com o mesmo nome/IDs da GPU real.
//! As fixtures são genéricas (sem dados de nenhuma máquina) e usam os bits reais de
//! `D3DKMT_ADAPTERTYPE`.
use hub_core::{
    machine::gpu_infos,
    sensors::{adapter_type::*, classify_adapter, physical_gpus, AdapterClass, Pci, RawAdapter},
    telemetry::{by_gpu, gpu_luid_map, gpu_usage, gpu_usage_grouped},
};

// Valores observados do campo AdapterType no Windows 11 (bitfield D3DKMT_ADAPTERTYPE).
const INTEGRATED: u32 = 0x232B; // Render | Display | Post | HybridIntegrated | SetTimings | RuntimePM
const DISCRETE: u32 = 0x233B; // idem com HybridDiscrete
const INDIRECT_DISPLAY: u32 = 0x342; // Display | IndirectDisplayDevice (sem Render)
const BASIC_RENDER: u32 = 0x105; // Render | SoftwareDevice

const MB: u64 = 1024 * 1024;
const GB: u64 = 1024 * MB;

fn pci(bus: u32, device: u32, function: u32) -> Option<Pci> {
    Some(Pci {
        bus,
        device,
        function,
    })
}
fn raw(
    luid: u64,
    name: &str,
    flags: Option<u32>,
    pci: Option<Pci>,
    dedicated: u64,
    shared: u64,
) -> RawAdapter {
    RawAdapter {
        luid,
        name: name.into(),
        dedicated: Some(dedicated).filter(|m| *m > 0),
        shared: Some(shared).filter(|m| *m > 0),
        flags,
        pci,
    }
}
fn igpu(luid: u64) -> RawAdapter {
    raw(
        luid,
        "Integrated Graphics",
        Some(INTEGRATED),
        pci(0, 2, 0),
        128 * MB,
        16 * GB,
    )
}
/// Display virtual: descrito como a GPU que renderiza para ele (mesmo nome, memória e IDs).
fn idd(luid: u64, n: u32) -> RawAdapter {
    raw(
        luid,
        "Integrated Graphics",
        Some(INDIRECT_DISPLAY),
        pci(1, 0, n),
        128 * MB,
        16 * GB,
    )
}
fn names(gpus: &[hub_core::sensors::Adapter]) -> Vec<&str> {
    gpus.iter().map(|g| g.name.as_str()).collect()
}

#[test]
fn adapter_type_bits_classify_physical_indirect_software_and_display_only() {
    assert_eq!(classify_adapter(INTEGRATED), AdapterClass::Physical);
    assert_eq!(classify_adapter(DISCRETE), AdapterClass::Physical);
    assert_eq!(classify_adapter(INDIRECT_DISPLAY), AdapterClass::Indirect);
    assert_eq!(classify_adapter(BASIC_RENDER), AdapterClass::Software);
    // Só tem saída de vídeo: não é GPU.
    assert_eq!(classify_adapter(DISPLAY), AdapterClass::DisplayOnly);
    assert_eq!(classify_adapter(0), AdapterClass::DisplayOnly);
    // Compute puro (aceleradora) e GPU paravirtualizada de VM são GPUs.
    assert_eq!(classify_adapter(COMPUTE_ONLY), AdapterClass::Physical);
    assert_eq!(
        classify_adapter(RENDER | PARAVIRTUALIZED),
        AdapterClass::Physical
    );
    // Software e indireto vencem mesmo com Render ligado.
    assert_eq!(
        classify_adapter(RENDER | SOFTWARE | INDIRECT_DISPLAY),
        AdapterClass::Software
    );
    assert_eq!(
        classify_adapter(RENDER | INDIRECT_DISPLAY),
        AdapterClass::Indirect
    );
}

#[test]
fn case_a_one_physical_gpu_plus_three_indirect_displays_is_one_gpu() {
    // A enumeração real da máquina em que o bug apareceu (LUIDs e nomes genéricos).
    let gpus = physical_gpus(vec![
        igpu(0x1526c),
        raw(
            0x1560b,
            "Microsoft Basic Render Driver",
            Some(BASIC_RENDER),
            None,
            0,
            16 * GB,
        ),
        idd(0x3c222, 1),
        idd(0x3539c, 2),
        idd(0x2c703, 3),
    ]);
    assert_eq!(gpus.len(), 1, "{gpus:?}");
    assert_eq!((gpus[0].luid, gpus[0].id.as_str()), (0x1526c, "pci:0:2.0"));
    assert_eq!(
        gpus[0].luids,
        vec![0x1526c],
        "o display virtual não vira fonte da GPU"
    );
    // O inventário (Machine Inventory) usa a mesma lista: uma entrada, não "+3".
    let inventory = gpu_infos(gpus);
    assert_eq!(inventory.len(), 1);
    assert_eq!(
        (inventory[0].name.as_str(), inventory[0].memory),
        ("Integrated Graphics", Some(128 * MB))
    );
}

#[test]
fn case_b_and_d_integrated_plus_discrete_are_two_gpus_in_a_stable_order() {
    // Equivalente genérico do PC de casa: iGPU + placa dedicada.
    let discrete = raw(
        0x9000,
        "Discrete Graphics",
        Some(DISCRETE),
        pci(1, 0, 0),
        4 * GB,
        8 * GB,
    );
    let gpus = physical_gpus(vec![discrete.clone(), igpu(0x1526c)]);
    assert_eq!(
        names(&gpus),
        vec!["Integrated Graphics", "Discrete Graphics"],
        "PCI menor primeiro"
    );
    assert_eq!(
        (gpus[0].id.as_str(), gpus[1].id.as_str()),
        ("pci:0:2.0", "pci:1:0.0")
    );
    // Mesmo com displays virtuais no meio, continuam duas.
    let mixed = physical_gpus(vec![idd(1, 1), discrete, idd(2, 2), igpu(0x1526c)]);
    assert_eq!(mixed.len(), 2);
    // Memória de cada uma intacta.
    assert_eq!(
        (mixed[0].dedicated, mixed[1].dedicated),
        (Some(128 * MB), Some(4 * GB))
    );
}

#[test]
fn case_c_two_identical_physical_gpus_remain_two_gpus() {
    // Mesmo nome, mesmos IDs, mesma memória: só o endereço PCI difere. Nada de agrupar por modelo.
    let card = |luid, bus| {
        raw(
            luid,
            "Discrete Graphics",
            Some(DISCRETE),
            pci(bus, 0, 0),
            8 * GB,
            16 * GB,
        )
    };
    let gpus = physical_gpus(vec![card(0xa001, 1), card(0xa002, 2), idd(0xb001, 1)]);
    assert_eq!(gpus.len(), 2, "{gpus:?}");
    assert_eq!(names(&gpus), vec!["Discrete Graphics", "Discrete Graphics"]);
    let ids: Vec<_> = gpus.iter().map(|g| g.id.clone()).collect();
    assert_eq!(ids, vec!["pci:1:0.0", "pci:2:0.0"]);
    assert_ne!(gpus[0].luid, gpus[1].luid);
    // Sem endereço PCI confiável, nada é fundido: na dúvida, nenhuma GPU some.
    let blind = |luid| {
        raw(
            luid,
            "Discrete Graphics",
            Some(DISCRETE),
            None,
            8 * GB,
            16 * GB,
        )
    };
    let gpus = physical_gpus(vec![blind(0xc001), blind(0xc002)]);
    assert_eq!(gpus.len(), 2);
    assert!(gpus.iter().all(|g| g.id.starts_with("luid:")));
}

#[test]
fn case_e_software_adapters_are_never_gpus() {
    let only_software = physical_gpus(vec![raw(
        7,
        "Microsoft Basic Render Driver",
        Some(BASIC_RENDER),
        None,
        0,
        GB,
    )]);
    assert!(only_software.is_empty());
    // Mesmo se o driver não informar o tipo, o nome do renderizador de software é reserva.
    let by_name = physical_gpus(vec![raw(
        8,
        "Microsoft Basic Render Driver",
        None,
        None,
        0,
        GB,
    )]);
    assert!(by_name.is_empty());
    // Com GPU real junto, só a real aparece.
    assert_eq!(
        physical_gpus(vec![
            igpu(1),
            raw(
                2,
                "Microsoft Basic Render Driver",
                Some(BASIC_RENDER),
                None,
                0,
                GB
            )
        ])
        .len(),
        1
    );
}

#[test]
fn case_f_remote_and_indirect_displays_never_create_a_gpu_card() {
    let only_virtual = physical_gpus(vec![idd(1, 1), idd(2, 2)]);
    assert!(only_virtual.is_empty());
    // A decisão é pelos bits, não pelo nome: um display indireto com o nome de uma GPU ainda é virtual.
    assert!(physical_gpus(vec![raw(
        3,
        "Qualquer Nome",
        Some(INDIRECT_DISPLAY),
        None,
        0,
        0
    )])
    .is_empty());
    // E uma GPU real cujo nome lembra "virtual" não é descartada quando o Windows informa que renderiza.
    assert_eq!(
        physical_gpus(vec![raw(
            4,
            "Virtual Display Capable GPU",
            Some(DISCRETE),
            pci(1, 0, 0),
            GB,
            GB
        )])
        .len(),
        1
    );
    // Só quando o tipo é desconhecido o nome serve de reserva.
    assert!(physical_gpus(vec![raw(5, "MS Idd Device", None, None, 0, 0)]).is_empty());
    assert_eq!(
        physical_gpus(vec![raw(6, "Discrete Graphics", None, None, GB, GB)]).len(),
        1
    );
    // Adaptador só de vídeo (sem render nem compute) também não é GPU.
    assert!(physical_gpus(vec![raw(
        9,
        "Display Only",
        Some(DISPLAY),
        pci(2, 0, 0),
        0,
        0
    )])
    .is_empty());
}

#[test]
fn case_g_several_luids_of_one_physical_gpu_are_one_gpu_with_every_telemetry_source() {
    // Dois LUIDs no mesmo endereço PCI (ex.: LUID novo após reset do driver ainda presente).
    let gpus = physical_gpus(vec![igpu(0x10), igpu(0x20), igpu(0x10)]);
    assert_eq!(gpus.len(), 1);
    assert_eq!(
        (gpus[0].luid, gpus[0].luids.clone()),
        (0x10, vec![0x10, 0x20])
    );

    let engine = |pid: u32, luid: u64, kind: &str| {
        format!("pid_{pid}_luid_0x00000000_0x{luid:08x}_phys_0_eng_0_engtype_{kind}")
    };
    let engines = vec![
        (engine(100, 0x10, "3D"), 30.0),
        (engine(101, 0x20, "3D"), 25.0), // mesmo tipo de motor em outro LUID da MESMA GPU: soma
        (engine(100, 0x10, "VideoDecode"), 10.0),
        (engine(102, 0x3c222, "3D"), 99.0), // LUID de display virtual: não é atividade desta GPU
    ];
    let luids = gpu_luid_map(&gpus);
    let (by_adapter, by_process) = gpu_usage_grouped(&engines, &luids);
    assert_eq!(
        by_adapter.get(&0x10).copied(),
        Some(55.0),
        "3D 30+25 > VideoDecode 10"
    );
    assert!(!by_adapter.contains_key(&0x20) && !by_adapter.contains_key(&0x3c222));
    // Por processo, o gasto total de cada um continua visível, inclusive no adaptador virtual.
    assert_eq!(by_process.get(&102).copied(), Some(99.0));
    assert_eq!(by_process.get(&100).copied(), Some(30.0));
    // Sem mapa (comportamento anterior), cada LUID continua por si.
    let (legacy, _) = gpu_usage(&engines);
    assert_eq!(legacy.get(&0x20).copied(), Some(25.0));
}

#[test]
fn memory_counters_sum_the_luids_of_one_gpu_and_ignore_virtual_ones() {
    let gpus = physical_gpus(vec![igpu(0x10), igpu(0x20), idd(0x3c222, 1)]);
    let luids = gpu_luid_map(&gpus);
    let inst = |luid: u64| format!("luid_0x00000000_0x{luid:08x}_phys_0");
    let shared = by_gpu(
        &[
            (inst(0x10), (2 * GB) as f64),
            (inst(0x20), GB as f64),
            (inst(0x3c222), 123.0),
        ],
        &luids,
    );
    assert_eq!(shared.get(&0x10).copied(), Some(3 * GB));
    assert_eq!(
        shared.len(),
        1,
        "o LUID do display virtual não vira memória de GPU"
    );
}

#[test]
fn dedicated_and_shared_memory_stay_separate_and_are_never_summed() {
    let gpus = physical_gpus(vec![igpu(1), idd(2, 1), idd(3, 2), idd(4, 3)]);
    assert_eq!(gpus.len(), 1);
    let gpu = &gpus[0];
    assert_eq!((gpu.dedicated, gpu.shared), (Some(128 * MB), Some(16 * GB)));
    // Entradas fundidas ficam com o MAIOR valor de cada segmento, nunca com a soma dos dois.
    let merged = physical_gpus(vec![
        raw(
            1,
            "Integrated Graphics",
            Some(INTEGRATED),
            pci(0, 2, 0),
            128 * MB,
            16 * GB,
        ),
        raw(
            2,
            "Integrated Graphics",
            Some(INTEGRATED),
            pci(0, 2, 0),
            0,
            16 * GB,
        ),
    ]);
    assert_eq!(
        (merged[0].dedicated, merged[0].shared),
        (Some(128 * MB), Some(16 * GB))
    );
    // Sem segmento dedicado informado, continua sem (nada de inventar a partir da compartilhada).
    let none = physical_gpus(vec![raw(
        1,
        "Integrated Graphics",
        Some(INTEGRATED),
        pci(0, 2, 0),
        0,
        16 * GB,
    )]);
    assert_eq!((none[0].dedicated, none[0].shared), (None, Some(16 * GB)));
}

#[test]
fn telemetry_attributes_activity_only_to_physical_gpus() {
    let gpus = physical_gpus(vec![
        igpu(0x1526c),
        idd(0x3c222, 1),
        idd(0x3539c, 2),
        idd(0x2c703, 3),
    ]);
    let luids = gpu_luid_map(&gpus);
    assert_eq!(luids.len(), 1);
    let engine = |pid: u32, luid: u64| {
        format!("pid_{pid}_luid_0x00000000_0x{luid:08x}_phys_0_eng_0_engtype_3D")
    };
    let (adapters, processes) = gpu_usage_grouped(&[(engine(7, 0x1526c), 18.0)], &luids);
    assert_eq!(
        adapters.get(&0x1526c).copied(),
        Some(18.0),
        "o uso real continua na iGPU"
    );
    assert_eq!(
        processes.get(&7).copied(),
        Some(18.0),
        "GPU por processo continua"
    );
}

#[test]
fn duplicated_luids_and_nameless_entries_are_ignored() {
    let gpus = physical_gpus(vec![
        igpu(1),
        igpu(1),
        raw(5, "", Some(INTEGRATED), pci(3, 0, 0), 0, 0),
    ]);
    assert_eq!(gpus.len(), 1);
    assert!(Pci {
        bus: 0,
        device: 2,
        function: 0
    }
    .valid());
    assert!(!Pci {
        bus: u32::MAX,
        device: 65535,
        function: 65535
    }
    .valid());
}

/// A máquina real (só Windows): nenhum display virtual/software vira GPU e as identidades são únicas.
#[cfg(windows)]
#[test]
fn real_enumeration_has_no_virtual_or_software_gpus_and_unique_identities() {
    use hub_core::sensors::gpu_adapters;
    let gpus = gpu_adapters();
    let mut ids: Vec<_> = gpus.iter().map(|g| g.id.clone()).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), gpus.len(), "id repetido: {gpus:?}");
    assert!(gpus
        .iter()
        .all(|g| !g.name.starts_with("Microsoft Basic") && g.luids.contains(&g.luid)));
    // Cada GPU tem LUIDs próprios: nenhum LUID pertence a duas GPUs.
    let mut all: Vec<u64> = gpus.iter().flat_map(|g| g.luids.clone()).collect();
    all.sort();
    let before = all.len();
    all.dedup();
    assert_eq!(all.len(), before);
}
