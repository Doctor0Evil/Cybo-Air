#pragma once
#include <string>
#include <vector>

namespace windnet {

struct WindnetRow {
    std::string nodeid;
    std::string assettype;
    std::string region;
    double lat;
    double lon;
    double avgwindspeed_ms;
    std::string dominantwind_from;
    std::string season;
    double netheight_m;
    double netwidth_m;
    double netporosity;
    double expected_capture_kg_per_year;
    int    maintenance_visits_per_year;
    double ecoimpactscore;
    double karmaperunit;
    std::string notes;
};

std::vector<WindnetRow> readCandidateCanyonsCSV(const std::string& path);
void writeWindnetShardCSV(const std::string& path,
                          const std::vector<WindnetRow>& rows);

} // namespace windnet
