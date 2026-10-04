// EA Trax renamer: replaces the entries of the game's song list (title, artist, album and
// where it plays) with the rows of NFSUServerChangerTrax.csv. Rows missing from the .csv keep
// the game's own entries.
#pragma once

#include <string>
#include <fstream>
#include <sstream>
#include <algorithm>
#include <cstring>
#include <cctype>
#include "includes\injector\injector.hpp"

const int traxRows = 26;
const int traxCols = 4;
// One entry in the game's song table: 4 string pointers.
// + 0*4 -> title, + 1*4 -> artist, +2*4 -> album, +3*4 -> play
const size_t traxOffset = 0x10;

// Reads up to traxRows lines of "play;title;artist;album". Returns the number of rows read;
// the rest of data stays untouched (nullptr).
static int loadCSV(const std::string& filename, char* data[traxRows][traxCols])
{
	std::ifstream file(filename);
	if (!file.is_open()) {
		return 0;
	}
	std::string line;
	int row = 0;
	while (std::getline(file, line) && row < traxRows) {
		std::stringstream ss(line);
		std::string value;
		int col = 0;
		while (std::getline(ss, value, ';') && col < traxCols) {
			if (value.empty()) {
				value = " ";
			}
			std::replace(value.begin(), value.end(), '^', ';');
			data[row][col] = new char[value.length() + 1];
			strcpy(data[row][col], value.c_str());
			col++;
		}
		while (col < traxCols) {
			data[row][col] = new char[2];
			strcpy(data[row][col], " ");
			col++;
		}
		row++;
	}
	return row;
}

// traxAddr: the game's song table; quoteTitles: put the titles in quotes like the game does.
// The strings are never freed: the game uses them for the whole run.
inline void ApplyTrax(uintptr_t traxAddr, const std::string& csvPath, bool quoteTitles)
{
	char* traxData[traxRows][traxCols] = {};
	int rows = loadCSV(csvPath, traxData);

	for (int i = 0; i < rows; i++) {

		char* traxTitle = traxData[i][1];
		char* traxArtist = traxData[i][2];
		char* traxAlbum = traxData[i][3];

		// The game's play codes; string literals live for the whole run, so the
		// pointers can go straight into the game's table.
		const char* traxPlay;
		switch (tolower(static_cast<unsigned char>(traxData[i][0][0]))) {
		case 'r': traxPlay = "IG"; break; // race
		case 'm': traxPlay = "FE"; break; // menu
		case 'a': traxPlay = "AL"; break; // all
		case 'x': traxPlay = "OF"; break; // off
		default:  traxPlay = i < 19 ? "IG" : "FE"; break;
		}

		if (quoteTitles) {
			char* quotedTitle = new char[strlen(traxTitle) + 3];
			strcpy(quotedTitle, "\"");
			strcat(quotedTitle, traxTitle);
			strcat(quotedTitle, "\"");
			traxTitle = quotedTitle;
		}

		const char* traxSet[4] = { traxTitle, traxArtist, traxAlbum, traxPlay };

		for (int j = 0; j < 4; ++j) {
			injector::WriteMemory(traxOffset * i + traxAddr + j * 4, traxSet[j], true);
		}
	}
}
