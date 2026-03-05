#pragma once
#include <algorithm>
#include <stdexcept>

namespace windnet {

struct NuisanceScores {
    double visual;      // 0 = no nuisance, 1 = very bad
    double safety;
    double maintenance;
};

inline double nuisancePenalty(const NuisanceScores& n) {
    const double wv = 0.4;
    const double ws = 0.4;
    const double wm = 0.2;
    const double avg = wv*n.visual + ws*n.safety + wm*n.maintenance;
    return std::clamp(avg, 0.0, 1.0);
}

inline double benefitFromMass(double Mcap_kg_per_year,
                              double Mref_kg_per_year) {
    if (Mref_kg_per_year <= 0.0) throw std::invalid_argument("Mref must be > 0");
    const double x = std::min(Mcap_kg_per_year / Mref_kg_per_year, 3.0);
    // Map [0,3] -> [0,1) with a convex rise
    return 1.0 - std::exp(-x);
}

inline double ecoImpactScore(double Mcap_kg_per_year,
                             double Mref_kg_per_year,
                             const NuisanceScores& n) {
    const double b = benefitFromMass(Mcap_kg_per_year, Mref_kg_per_year);
    const double pen = nuisancePenalty(n);
    const double w_b = 0.8;
    const double w_p = 0.2;
    double score = w_b * b * (1.0 - pen);
    return std::clamp(score, 0.0, 1.0);
}

} // namespace windnet
