//! Measures of a stretch of sound, for comparing an approximated analogue circuit with MAME's
//! model of it: the level (RMS), the strongest frequency, and the spectrum's shape in
//! twelfth-octave and (averaged over short windows) octave bands. tests/galaxian/sound.rs has
//! the 192 kHz version these follow.

#![allow(dead_code)]

#[derive(Clone, Debug, PartialEq)]
pub struct Measure {
    /// RMS less the mean, as a fraction of full scale.
    pub rms: f64,
    /// The strongest frequency between 40 Hz and 12 kHz.
    pub freq: f64,
    /// Energy in twelfth-octave bands from 40 Hz (100 of them).
    pub fine: Vec<f64>,
    /// Energy in octave bands from 40 Hz (8), averaged over windows of a twelfth of the rate.
    pub octaves: Vec<f64>,
}

pub fn measure(samples: &[i16], rate: f64) -> Measure {
    let v: Vec<f64> = samples.iter().map(|&s| f64::from(s) / 32767.0).collect();
    let mean = v.iter().sum::<f64>() / v.len().max(1) as f64;
    let rms = (v.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / v.len().max(1) as f64).sqrt();
    const N: usize = 65_536;
    let len = v.len().min(N);
    let mut re = vec![0.0; N];
    for (i, r) in re.iter_mut().enumerate().take(len) {
        let hann = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / len as f64).cos();
        *r = (v[i] - mean) * hann;
    }
    let mut im = vec![0.0; N];
    fft(&mut re, &mut im);
    let mag: Vec<f64> = re.iter().zip(&im).map(|(r, i)| (r * r + i * i).sqrt()).collect();
    let bin = rate / N as f64;
    let (lo, hi) = ((40.0 / bin) as usize, ((12_000.0f64).min(rate / 2.0 - 1.0) / bin) as usize);
    let peak = (lo..hi).max_by(|&a, &b| mag[a].total_cmp(&mag[b])).unwrap();
    let (a, b, c) = (mag[peak - 1].max(1e-12).ln(), mag[peak].max(1e-12).ln(), mag[peak + 1].max(1e-12).ln());
    let shift = if a - 2.0 * b + c != 0.0 { 0.5 * (a - c) / (a - 2.0 * b + c) } else { 0.0 };
    let fine = (0..100)
        .map(|band| {
            let f0 = 40.0 * 2f64.powf(band as f64 / 12.0);
            let f1 = 40.0 * 2f64.powf((band + 1) as f64 / 12.0);
            let b0 = ((f0 / bin) as usize).min(N / 2 - 1);
            let b1 = ((f1 / bin) as usize).max(b0 + 1).min(N / 2);
            mag[b0..b1].iter().map(|m| m * m).sum::<f64>()
        })
        .collect();
    Measure { rms, freq: (peak as f64 + shift) * bin, fine, octaves: octaves(&v, mean, rate) }
}

fn octaves(v: &[f64], mean: f64, rate: f64) -> Vec<f64> {
    let w = ((rate / 12.0) as usize).next_power_of_two();
    let mut power = vec![0.0; w / 2];
    let mut windows = 0u32;
    let mut start = 0;
    while start + w <= v.len() {
        let mut re: Vec<f64> = (0..w)
            .map(|i| (v[start + i] - mean) * (0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / w as f64).cos()))
            .collect();
        let mut im = vec![0.0; w];
        fft(&mut re, &mut im);
        for (p, (r, i)) in power.iter_mut().zip(re.iter().zip(&im)) {
            *p += r * r + i * i;
        }
        windows += 1;
        start += w / 2;
    }
    let bin = rate / w as f64;
    (0..8)
        .map(|o| {
            let (f0, f1) = (40.0 * f64::from(1u32 << o), 40.0 * f64::from(2u32 << o));
            let b0 = ((f0 / bin).round() as usize).min(w / 2 - 1);
            let b1 = ((f1 / bin).round() as usize).clamp(b0 + 1, w / 2);
            power[b0..b1].iter().sum::<f64>() / f64::from(windows.max(1))
        })
        .collect()
}

/// How alike two spectra are in shape: the cosine of the angle between them (1 is the same).
pub fn likeness(a: &[f64], b: &[f64]) -> f64 {
    let dot: f64 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na: f64 = a.iter().map(|x| x * x).sum::<f64>().sqrt();
    let nb: f64 = b.iter().map(|x| x * x).sum::<f64>().sqrt();
    if na == 0.0 || nb == 0.0 { 0.0 } else { dot / (na * nb) }
}

pub fn fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * std::f64::consts::PI / len as f64;
        for start in (0..n).step_by(len) {
            for k in 0..len / 2 {
                let (s, c) = (ang * k as f64).sin_cos();
                let (a, b) = (start + k, start + k + len / 2);
                let (tr, ti) = (re[b] * c - im[b] * s, re[b] * s + im[b] * c);
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
            }
        }
        len <<= 1;
    }
}
