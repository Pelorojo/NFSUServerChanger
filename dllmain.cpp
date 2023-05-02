#include "stdafx.h"
#include "stdio.h"
#include <string>
#include <windows.h>
#include "includes\injector\injector.hpp"
#include "includes\IniReader.h"

#include <iostream>
using namespace std;
#include <fstream>

const int traxRows = 26;
const int traxCols = 4;
const size_t traxOffset = 0x10;

const char* defaultSrv = "ps2nfs04.ea.com";

const uintptr_t serverAddr = 0x6F1F40;
const uintptr_t acceptAddr = 0x734F54;

// Base mem addr: title, artist, album, play
const uintptr_t traxAddr[traxCols] = { 0x6F4D08, 0x6F4D0C, 0x6F4D10, 0x6F4D14 };

const char* iniFile = "NFSUServerChanger.ini";
const char* csvFile = "NFSUServerChangerTrax.csv";

bool fileExists(const string& filename)
{
	ifstream file(filename);
	return file.good();
}

void loadCSV(const char* filename, char* data[traxRows][traxCols])
{
	ifstream file(filename);
	if (!file.is_open()) {
		cout << "Failed to open file!" << endl;
		return;
	}
	string line;
	int row = 0;
	while (getline(file, line) && row < traxRows) {
		stringstream ss(line);
		string value;
		int col = 0;
		while (getline(ss, value, ';') && col < traxCols) {
			if (value.empty()) {
				value = " ";
			}
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
	file.close();
}

int Init()
{
	CIniReader iniReader(iniFile);

	// EATrax
	if (fileExists(csvFile)) {

		char* noQuotes = iniReader.ReadString("EATrax", "NoQuotes", "0");
		char* traxData[traxRows][traxCols];
		loadCSV(csvFile, traxData);

		for (int i = 0; i < traxRows; i++) {

			char* traxPlay = traxData[i][0];
			char* traxTitle = traxData[i][1];
			char* traxArtist = traxData[i][2];
			char* traxAlbum = traxData[i][3];

			if (tolower(traxPlay[0]) == 'r') {
				strcpy(traxPlay, "IG");
			}
			else if (tolower(traxPlay[0]) == 'm') {
				strcpy(traxPlay, "FE");
			}
			else if (tolower(traxPlay[0]) == 'a') {
				strcpy(traxPlay, "AL");
			}
			else {
				if (i < 19) {
					strcpy(traxPlay, "IG");
				}
				else {
					strcpy(traxPlay, "FE");
				}
			}

			if (strcmp(noQuotes, "1") != 0) {
				char* quotedTitle = new char[strlen(traxTitle) + 3];
				strcpy(quotedTitle, "\"");
				strcat(quotedTitle, traxTitle);
				strcat(quotedTitle, "\"");
				traxTitle = quotedTitle;
			}

			injector::WriteMemory(traxOffset * i + traxAddr[0], traxTitle, true);
			injector::WriteMemory(traxOffset * i + traxAddr[1], traxArtist, true);
			injector::WriteMemory(traxOffset * i + traxAddr[2], traxAlbum, true);
			injector::WriteMemory(traxOffset * i + traxAddr[3], traxPlay, true);
		}
	}

	// GameServer
	typedef void* memory_pointer_tr;
	char* gameServer = iniReader.ReadString("Server", "Host", defaultSrv);

	if (strcmp(gameServer, defaultSrv) != 0) {

		char gameServerLtd[33];
		strncpy(gameServerLtd, gameServer, 32);
		gameServerLtd[32] = '\0';

		memory_pointer_tr memAddr = reinterpret_cast<memory_pointer_tr>(serverAddr);
		injector::WriteMemoryRaw(memAddr, gameServerLtd, strlen(gameServerLtd) + 1, true);
	}

	memory_pointer_tr memAddr = reinterpret_cast<memory_pointer_tr>(acceptAddr);
	injector::MemoryFill(memAddr, 1, 1, true);

	return 0;
}

BOOL APIENTRY DllMain(HMODULE /*hModule*/, DWORD reason, LPVOID /*lpReserved*/)
{
	if (reason == DLL_PROCESS_ATTACH)
	{
		uintptr_t base = (uintptr_t)GetModuleHandleA(NULL);
		IMAGE_DOS_HEADER* dos = (IMAGE_DOS_HEADER*)(base);
		IMAGE_NT_HEADERS* nt = (IMAGE_NT_HEADERS*)(base + dos->e_lfanew);

		if ((base + nt->OptionalHeader.AddressOfEntryPoint + (0x400000 - base)) == 0x670CB5) // Check if .exe file is compatible - Thanks to thelink2012 and MWisBest
			Init();

		else
		{
			MessageBoxA(NULL, "This .exe is not supported.\nPlease use v1.4 English speed.exe (3,03 MB (3.178.496 bytes)).", "NFSU Server Changer", MB_ICONERROR);
			return FALSE;
		}
	}
	return TRUE;

}

