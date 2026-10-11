use anyhow::{ensure, Result};
use serde_json::{json, Value};

/// PCA of complete finite rows, centered and optionally standardized by sample variance.
pub fn pca(rows: &[Vec<f64>], standardize: bool) -> Result<Value> {
    ensure!(rows.len() >= 2, "PCA needs at least two complete rows");
    let width = rows[0].len();
    ensure!(
        (2..=32).contains(&width),
        "Select between 2 and 32 numeric features"
    );
    ensure!(
        rows.iter()
            .all(|row| row.len() == width && row.iter().all(|value| value.is_finite())),
        "PCA rows must have finite numeric values"
    );
    let count = rows.len() as f64;
    let means: Vec<f64> = (0..width)
        .map(|column| rows.iter().map(|row| row[column]).sum::<f64>() / count)
        .collect();
    let variances: Vec<f64> = (0..width)
        .map(|column| {
            rows.iter()
                .map(|row| (row[column] - means[column]).powi(2))
                .sum::<f64>()
                / (count - 1.0)
        })
        .collect();
    ensure!(
        variances.iter().any(|value| *value > 0.0),
        "Selected features are constant; there is no distribution to project"
    );
    let scales: Vec<f64> = variances
        .iter()
        .map(|variance| {
            if standardize && *variance > 0.0 {
                variance.sqrt()
            } else {
                1.0
            }
        })
        .collect();
    let centered: Vec<Vec<f64>> = rows
        .iter()
        .map(|row| {
            (0..width)
                .map(|column| (row[column] - means[column]) / scales[column])
                .collect()
        })
        .collect();
    let mut covariance = vec![vec![0.0; width]; width];
    for a in 0..width {
        for b in a..width {
            let value = centered.iter().map(|row| row[a] * row[b]).sum::<f64>() / (count - 1.0);
            covariance[a][b] = value;
            covariance[b][a] = value;
        }
    }
    ensure!(
        covariance.iter().flatten().all(|value| value.is_finite()),
        "Feature magnitudes are too large for PCA"
    );
    let mut vectors = vec![vec![0.0; width]; width];
    for (index, row) in vectors.iter_mut().enumerate() {
        row[index] = 1.0;
    }
    let tolerance = covariance
        .iter()
        .enumerate()
        .map(|(i, row)| row[i].abs())
        .fold(0.0, f64::max)
        * 1e-12;
    for _ in 0..(width * width * 50) {
        let mut pivot = (0, 1);
        let mut largest = 0.0;
        for (a, row) in covariance.iter().enumerate() {
            for (b, value) in row.iter().enumerate().skip(a + 1) {
                if value.abs() > largest {
                    largest = value.abs();
                    pivot = (a, b);
                }
            }
        }
        if largest <= tolerance {
            break;
        }
        let (a, b) = pivot;
        let tau = (covariance[b][b] - covariance[a][a]) / (2.0 * covariance[a][b]);
        let tangent = tau.signum() / (tau.abs() + (1.0 + tau * tau).sqrt());
        let tangent = if tau == 0.0 { 1.0 } else { tangent };
        let cosine = 1.0 / (1.0 + tangent * tangent).sqrt();
        let sine = tangent * cosine;
        let cross = covariance[a][b];
        covariance[a][a] -= tangent * cross;
        covariance[b][b] += tangent * cross;
        covariance[a][b] = 0.0;
        covariance[b][a] = 0.0;
        let rotated: Vec<_> = covariance
            .iter()
            .enumerate()
            .filter(|(k, _)| *k != a && *k != b)
            .map(|(k, row)| {
                (
                    k,
                    cosine * row[a] - sine * row[b],
                    sine * row[a] + cosine * row[b],
                )
            })
            .collect();
        for (k, left, right) in rotated {
            covariance[k][a] = left;
            covariance[a][k] = left;
            covariance[k][b] = right;
            covariance[b][k] = right;
        }
        for row in &mut vectors {
            let left = row[a];
            let right = row[b];
            row[a] = cosine * left - sine * right;
            row[b] = sine * left + cosine * right;
        }
    }
    let mut order: Vec<usize> = (0..width).collect();
    order.sort_by(|a, b| covariance[*b][*b].total_cmp(&covariance[*a][*a]));
    let eigenvalues: Vec<f64> = order.iter().map(|i| covariance[*i][*i].max(0.0)).collect();
    let total = eigenvalues.iter().sum::<f64>();
    let mut loadings: Vec<Vec<f64>> = order
        .iter()
        .take(2)
        .map(|i| vectors.iter().map(|row| row[*i]).collect())
        .collect();
    for direction in &mut loadings {
        let largest = direction
            .iter()
            .max_by(|a, b| a.abs().total_cmp(&b.abs()))
            .unwrap();
        if *largest < 0.0 {
            for value in direction {
                *value = -*value;
            }
        }
    }
    let scores: Vec<Vec<f64>> = centered
        .iter()
        .map(|row| {
            loadings
                .iter()
                .map(|direction| {
                    row.iter()
                        .zip(direction)
                        .map(|(value, loading)| value * loading)
                        .sum()
                })
                .collect()
        })
        .collect();
    Ok(
        json!({"scores":scores,"explained_variance_ratio":[eigenvalues[0]/total,eigenvalues[1]/total],"loadings":loadings,"means":means,"scales":scales,"constant_features":variances.iter().enumerate().filter(|(_, value)| **value == 0.0).map(|(i, _)| i).collect::<Vec<_>>()}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn correlated_features_have_one_component_and_centered_scores() {
        let result = pca(&[vec![1.0, 2.0], vec![2.0, 4.0], vec![3.0, 6.0]], true).unwrap();
        assert!((result["explained_variance_ratio"][0].as_f64().unwrap() - 1.0).abs() < 1e-10);
        assert!(result["scores"][1][0].as_f64().unwrap().abs() < 1e-10);
        assert!(result["scores"][0][1].as_f64().unwrap().abs() < 1e-10);
    }
    #[test]
    fn standardized_projection_is_invariant_to_units() {
        let rows = vec![
            vec![1.0, 4.0],
            vec![2.0, 2.0],
            vec![4.0, 3.0],
            vec![5.0, 8.0],
        ];
        let scaled: Vec<_> = rows
            .iter()
            .map(|row| vec![row[0] * 1000.0, row[1]])
            .collect();
        let a = pca(&rows, true).unwrap();
        let b = pca(&scaled, true).unwrap();
        for index in 0..2 {
            assert!(
                (a["explained_variance_ratio"][index].as_f64().unwrap()
                    - b["explained_variance_ratio"][index].as_f64().unwrap())
                .abs()
                    < 1e-10
            );
        }
        assert!(pca(&[vec![1.0, 1.0], vec![1.0, 1.0]], true).is_err());
    }
    #[test]
    fn independent_axes_match_known_covariance_eigenvalues() {
        let rows = vec![
            vec![1.0, 0.0, 0.0],
            vec![-1.0, 0.0, 0.0],
            vec![0.0, 2.0, 0.0],
            vec![0.0, -2.0, 0.0],
            vec![0.0, 0.0, 3.0],
            vec![0.0, 0.0, -3.0],
        ];
        let result = pca(&rows, false).unwrap();
        assert!(
            (result["explained_variance_ratio"][0].as_f64().unwrap() - 9.0 / 14.0).abs() < 1e-10
        );
        assert!(
            (result["explained_variance_ratio"][1].as_f64().unwrap() - 2.0 / 7.0).abs() < 1e-10
        );
        assert_eq!(result["loadings"][0], json!([0.0, 0.0, 1.0]));
    }
}
