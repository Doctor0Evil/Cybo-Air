use super::{BeeCorridorPolytope, BeeKarma, BeeKarmaEnvelope, BeeStressorState};

pub trait BeeAdmissible {
    fn bee_state(&self) -> &BeeStressorState;
    fn bee_corridor(&self) -> &BeeCorridorPolytope;
    fn bee_karma(&self) -> BeeKarma;

    fn is_inside_polytope(&self) -> bool {
        let x = self.bee_state();
        let vec_x = vec![
            x.hq_pest,
            x.h_rf,
            x.h_poll,
            x.d_h_bio,
            x.varroa_per_100,
            x.d_thive_c,
            1.0 - x.q_forage, // convert success into "stress"
        ];
        let p = self.bee_corridor();
        for (row, &b_i) in p.a.iter().zip(p.b.iter()) {
            let dot = row.iter().zip(vec_x.iter()).map(|(a, v)| a * v).sum::<f64>();
            if dot > b_i {
                return false;
            }
        }
        true
    }

    fn is_bee_admissible(&self) -> bool {
        self.is_inside_polytope() && self.bee_karma().0 >= self.bee_corridor().kappa_min
    }
}

pub trait BloodGated {
    fn envelope(&self) -> &BeeKarmaEnvelope;
    fn envelope_mut(&mut self) -> &mut BeeKarmaEnvelope;

    fn apply_karma_delta(&mut self, delta: f64) {
        let env = self.envelope_mut();
        let mut k = env.kappa.0 + delta;
        if k > 1.0 {
            k = 1.0;
        }
        if k < 0.0 {
            k = 0.0;
        }
        env.kappa = BeeKarma(k);
        env.blood_gate_level = if k >= 0.8 {
            3
        } else if k >= 0.6 {
            2
        } else if k >= 0.4 {
            1
        } else {
            0
        };
    }
}
